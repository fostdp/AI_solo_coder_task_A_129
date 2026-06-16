#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
古代铜鼓铸造工艺仿真与声学特性分析系统
铜鼓工艺模拟器脚本
========================================
功能：
1. 每小时生成模拟传感器数据（合金成分、壁厚分布、敲击音频谱）
2. 通过HTTP POST发送到后端 /api/sensor/readings
3. 可选地通过MQTT发布到 bronze-drum/sensors/{drum_id}
4. 支持引入随机扰动以模拟真实测量噪声
5. 可手动触发铸造缺陷事件和音准漂移事件

用法：
  python sensor_simulator.py                      # 默认模式，每小时上报
  python sensor_simulator.py --interval 60        # 每60秒上报一次（调试）
  python sensor_simulator.py --drum drum-001-zhuang  # 只模拟特定鼓
  python sensor_simulator.py --inject-failure      # 注入一个异常事件
  python sensor_simulator.py --mqtt                # 同时启用MQTT发布
"""

import argparse
import json
import math
import random
import sys
import time
import uuid
from datetime import datetime, timezone
from typing import Dict, List, Optional, Tuple

import requests

try:
    import paho.mqtt.client as mqtt
    HAS_MQTT = True
except ImportError:
    HAS_MQTT = False


API_BASE = "http://127.0.0.1:8080"
MQTT_HOST = "127.0.0.1"
MQTT_PORT = 1883
MQTT_SENSOR_TOPIC = "bronze-drum/sensors"

# 西南少数民族典型铜鼓档案
DEFAULT_DRUMS = [
    {
        "drum_id": "drum-001-zhuang",
        "name": "左江花山型铜鼓",
        "ethnic": "壮族",
        "diameter_cm": 78.5,
        "height_cm": 52.3,
        "base_alloy": {"copper": 77.8, "tin": 18.3, "lead": 3.2, "zinc": 0.4, "other": 0.3},
        "base_thickness_mm": 6.0,
        "resonance_freqs": [523.25, 659.25, 783.99, 1046.50, 1318.51],
    },
    {
        "drum_id": "drum-002-miao",
        "name": "雷山型大铜鼓",
        "ethnic": "苗族",
        "diameter_cm": 112.0,
        "height_cm": 76.0,
        "base_alloy": {"copper": 75.5, "tin": 20.1, "lead": 3.8, "zinc": 0.3, "other": 0.3},
        "base_thickness_mm": 8.5,
        "resonance_freqs": [392.0, 493.88, 587.33, 783.99, 987.77],
    },
    {
        "drum_id": "drum-003-dong",
        "name": "从江侗族铜鼓",
        "ethnic": "侗族",
        "diameter_cm": 56.2,
        "height_cm": 38.0,
        "base_alloy": {"copper": 79.5, "tin": 17.2, "lead": 2.5, "zinc": 0.5, "other": 0.3},
        "base_thickness_mm": 4.5,
        "resonance_freqs": [659.25, 783.99, 987.77, 1174.66, 1396.91],
    },
    {
        "drum_id": "drum-004-bouyei",
        "name": "黔南布依铜鼓",
        "ethnic": "布依族",
        "diameter_cm": 65.8,
        "height_cm": 44.5,
        "base_alloy": {"copper": 78.2, "tin": 17.8, "lead": 3.3, "zinc": 0.4, "other": 0.3},
        "base_thickness_mm": 5.2,
        "resonance_freqs": [587.33, 698.46, 880.00, 1046.50, 1318.51],
    },
    {
        "drum_id": "drum-005-yi",
        "name": "楚雄万家坝型",
        "ethnic": "彝族",
        "diameter_cm": 45.0,
        "height_cm": 28.0,
        "base_alloy": {"copper": 82.0, "tin": 15.5, "lead": 1.8, "zinc": 0.4, "other": 0.3},
        "base_thickness_mm": 3.8,
        "resonance_freqs": [698.46, 880.00, 1046.50, 1318.51, 1567.98],
    },
]

ZONES = [
    "鼓心/太阳纹区", "主晕圈/羽人纹区", "主晕圈/羽人纹区",
    "鼓面外圈/立蛙区", "鼓面外圈/立蛙区", "鼓腰/胴部",
    "鼓腰/胴部", "鼓足/底部边缘", "鼓足/底部边缘",
    "耳部/纹饰区", "鼓心/太阳纹区", "鼓面外圈/立蛙区",
]


class GaussianNoise:
    @staticmethod
    def normal(mean: float, std: float) -> float:
        return random.gauss(mean, std)


def generate_alloy(base: Dict, drift_event: bool = False) -> Dict:
    """模拟合金成分测量，含X射线荧光光谱仪的典型误差"""
    noise = 0.12
    result = {
        "copper_pct": round(GaussianNoise.normal(base["copper"], noise), 2),
        "tin_pct": round(GaussianNoise.normal(base["tin"], noise * 0.8), 2),
        "lead_pct": round(GaussianNoise.normal(base["lead"], noise * 0.5), 2),
        "zinc_pct": round(abs(GaussianNoise.normal(base["zinc"], noise * 0.3)), 2),
        "other_impurities_pct": round(abs(GaussianNoise.normal(base["other"], noise * 0.2)), 2),
    }

    if drift_event:
        result["tin_pct"] = round(base["tin"] - random.uniform(2.0, 4.5), 2)
        result["copper_pct"] = round(base["copper"] + random.uniform(1.5, 3.5), 2)

    total = sum(result.values())
    if abs(total - 100.0) > 0.5:
        scale = 100.0 / total
        for k in result:
            result[k] = round(result[k] * scale, 2)

    return result


def generate_wall_thickness(base_thickness_mm: float, diameter_cm: float,
                             anomaly: bool = False) -> List[Dict]:
    """生成12个壁厚测点，模拟鼓形几何特征与测量误差"""
    points = []
    for i, zone in enumerate(ZONES):
        angle = (i / len(ZONES)) * 2 * math.pi
        x_frac = 0.5 + 0.4 * math.cos(angle) * random.uniform(0.85, 1.15)
        y_frac = 0.5 + 0.4 * math.sin(angle) * random.uniform(0.85, 1.15)

        r = math.sqrt((x_frac - 0.5) ** 2 + (y_frac - 0.5) ** 2) * 2
        geometry_factor = 1.0 + 0.15 * math.sin(r * math.pi * 2) - 0.1 * abs(r - 0.5)

        thickness = base_thickness_mm * geometry_factor * random.uniform(0.92, 1.08)

        if anomaly and random.random() < 0.25:
            thickness *= random.uniform(0.55, 0.75)

        points.append({
            "zone": zone,
            "x_frac": round(x_frac, 4),
            "y_frac": round(y_frac, 4),
            "thickness_mm": round(thickness, 3),
        })

    return points


def generate_tap_spectrum(resonance_freqs: List[float], freq_drift: bool = False) -> List[Dict]:
    """生成敲击音频谱，含参考共振频率、各次谐波、以及背景噪声"""
    spectrum = []
    start_freq = 50.0
    end_freq = 3000.0
    bin_count = 256
    step = (end_freq - start_freq) / bin_count

    drift_hz = random.uniform(-8.0, 8.0) if freq_drift else random.uniform(-1.5, 1.5)
    damping_rand = random.uniform(0.8, 1.2)

    for i in range(bin_count):
        freq = start_freq + i * step
        amplitude = -60.0 - random.uniform(0, 5)

        for harmonic_order, base_f in enumerate(resonance_freqs[:5], 1):
            shifted = base_f * (1 + drift_hz / base_f * harmonic_order)
            width = 3.0 + 2.0 * harmonic_order
            distance = abs(freq - shifted)
            if distance < 30:
                peak = 12.0 - 3.5 * harmonic_order - (distance / width) ** 2 * 6
                peak *= damping_rand
                amplitude = max(amplitude, peak)

        amplitude += random.uniform(-1.5, 1.5)
        spectrum.append({
            "frequency_hz": round(freq, 2),
            "amplitude_db": round(max(amplitude, -80.0), 2),
        })

    return spectrum


def build_sensor_reading(drum_cfg: Dict, failure_event: bool = False) -> Dict:
    """组装一次完整的传感器读数"""
    now = datetime.now(timezone.utc)
    reading_id = str(uuid.uuid4())

    anomaly_type = None
    if failure_event:
        anomaly_type = random.choice(["alloy", "thickness", "frequency", "all"])

    alloy = generate_alloy(
        drum_cfg["base_alloy"],
        drift_event=(anomaly_type in ("alloy", "all")),
    )

    wall = generate_wall_thickness(
        drum_cfg["base_thickness_mm"],
        drum_cfg["diameter_cm"],
        anomaly=(anomaly_type in ("thickness", "all")),
    )

    spectrum = generate_tap_spectrum(
        drum_cfg["resonance_freqs"],
        freq_drift=(anomaly_type in ("frequency", "all")),
    )

    return {
        "reading_id": reading_id,
        "drum_id": drum_cfg["drum_id"],
        "timestamp": now.isoformat(),
        "alloy": alloy,
        "wall_thickness": wall,
        "tap_spectrum": spectrum,
        "temperature_c": round(GaussianNoise.normal(24.0, 1.8), 1),
        "ambient_humidity_pct": round(GaussianNoise.normal(55.0, 8.0), 1),
        "sensor_ids": [
            f"XRF-{random.randint(100, 999)}",
            f"UT-{random.randint(1000, 9999)}",
            f"MIC-{random.randint(10000, 99999)}",
            f"ENV-{random.randint(1000, 9999)}",
        ],
    }


def send_http(reading: Dict, timeout: float = 30.0) -> Tuple[bool, Optional[str]]:
    """通过HTTP API上报读数"""
    url = f"{API_BASE}/api/sensor/readings"
    try:
        resp = requests.post(url, json=reading, timeout=timeout)
        if resp.status_code == 200:
            data = resp.json()
            alarms = data.get("data") or []
            if alarms:
                print(f"  ⚠️  本次读数触发 {len(alarms)} 条告警:")
                for a in alarms[:3]:
                    print(f"    - [{a.get('severity')}] {a.get('alarm_type')}: {a.get('message')[:60]}")
            return True, None
        else:
            return False, f"HTTP {resp.status_code}: {resp.text[:200]}"
    except Exception as e:
        return False, str(e)


def send_mqtt(mqtt_client: "mqtt.Client", reading: Dict, drum_id: str) -> bool:
    """通过MQTT发布读数"""
    try:
        topic = f"{MQTT_SENSOR_TOPIC}/{drum_id}"
        payload = json.dumps(reading, ensure_ascii=False).encode("utf-8")
        result = mqtt_client.publish(topic, payload, qos=1)
        return result.rc == 0
    except Exception as e:
        print(f"  MQTT发布失败: {e}", file=sys.stderr)
        return False


def ensure_drums_registered():
    """如果后端正没有鼓档案，则先初始化"""
    try:
        resp = requests.get(f"{API_BASE}/api/drums", timeout=10)
        if resp.status_code == 200:
            data = resp.json()
            existing = {d["drum_id"] for d in (data.get("data") or [])}
            for drum in DEFAULT_DRUMS:
                if drum["drum_id"] not in existing:
                    print(f"注册铜鼓档案: {drum['name']} ({drum['drum_id']})")
                    body = {
                        "name": drum["name"],
                        "ethnic_group": drum["ethnic"],
                        "origin_region": f"模拟-{drum['ethnic']}地区",
                        "estimated_era": "模拟数据-现代复原",
                        "diameter_cm": drum["diameter_cm"],
                        "height_cm": drum["height_cm"],
                        "mass_kg": round(drum["diameter_cm"] * 0.55, 1),
                        "notes": "传感器模拟器自动创建的模拟铜鼓档案",
                    }
                    requests.post(f"{API_BASE}/api/drums", json=body, timeout=10)
    except Exception as e:
        print(f"检查/注册铜鼓档案失败 (可能后端未启动): {e}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description="铜鼓工艺模拟器")
    parser.add_argument("--interval", type=int, default=3600,
                        help="上报间隔秒数（默认3600=1小时）")
    parser.add_argument("--drum", type=str, default=None,
                        help="只模拟指定drum_id")
    parser.add_argument("--inject-failure", action="store_true",
                        help="单次注入异常事件后退出")
    parser.add_argument("--run-once", action="store_true",
                        help="只发送一轮读数后退出")
    parser.add_argument("--mqtt", action="store_true",
                        help="同时启用MQTT发布")
    parser.add_argument("--api", type=str, default=API_BASE,
                        help=f"后端API地址 (默认 {API_BASE})")
    parser.add_argument("--verbose", action="store_true",
                        help="显示详细日志")
    args = parser.parse_args()

    global API_BASE
    API_BASE = args.api

    drums = [d for d in DEFAULT_DRUMS if args.drum is None or d["drum_id"] == args.drum]
    if not drums:
        print(f"未找到匹配的铜鼓: {args.drum}")
        sys.exit(1)

    ensure_drums_registered()

    mqtt_client = None
    if args.mqtt:
        if not HAS_MQTT:
            print("⚠️  未安装paho-mqtt，无法启用MQTT (pip install paho-mqtt)")
        else:
            mqtt_client = mqtt.Client(client_id=f"simulator-{uuid.uuid4().hex[:8]}")
            try:
                mqtt_client.connect(MQTT_HOST, MQTT_PORT, keepalive=60)
                mqtt_client.loop_start()
                print(f"✅ MQTT已连接 {MQTT_HOST}:{MQTT_PORT}")
            except Exception as e:
                print(f"⚠️  MQTT连接失败: {e}，仅使用HTTP")
                mqtt_client = None

    print(f"\n🛢️  铜鼓工艺模拟器启动")
    print(f"   监控铜鼓: {len(drums)} 面")
    print(f"   上报间隔: {args.interval} 秒 ({args.interval/60:.1f} 分钟)")
    print(f"   API地址:  {API_BASE}")
    print(f"   MQTT:     {'启用' if mqtt_client else '禁用'}")
    print(f"   模式:     {'异常注入' if args.inject_failure else ('单次运行' if args.run_once else '循环运行')}")
    print("=" * 60 + "\n")

    while True:
        round_start = time.time()
        print(f"[{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}] 开始新的采集轮次")

        for idx, drum in enumerate(drums, 1):
            should_fail = args.inject_failure and idx == len(drums)
            reading = build_sensor_reading(drum, failure_event=should_fail)

            alloy = reading["alloy"]
            avg_t = sum(p["thickness_mm"] for p in reading["wall_thickness"]) / len(reading["wall_thickness"])
            print(f"  [{idx}/{len(drums)}] {drum['name']} ({drum['drum_id']})")
            if args.verbose:
                print(f"    合金: Cu={alloy['copper_pct']}%, Sn={alloy['tin_pct']}%, Pb={alloy['lead_pct']}%")
                print(f"    平均壁厚: {avg_t:.2f}mm, 谱点: {len(reading['tap_spectrum'])}")
                print(f"    温湿度: {reading['temperature_c']}℃ / {reading['ambient_humidity_pct']}%")

            ok, err = send_http(reading)
            if ok:
                print(f"    ✅ HTTP上报成功")
            else:
                print(f"    ❌ HTTP上报失败: {err}", file=sys.stderr)

            if mqtt_client:
                if send_mqtt(mqtt_client, reading, drum["drum_id"]):
                    print(f"    ✅ MQTT已发布")

            time.sleep(0.5)

        elapsed = time.time() - round_start
        print(f"本轮完成，耗时 {elapsed:.1f} 秒\n")

        if args.inject_failure:
            print("异常注入模式已完成，退出。")
            break
        if args.run_once:
            print("单次运行模式，退出。")
            break

        sleep_time = max(1.0, args.interval - elapsed)
        print(f"等待 {sleep_time:.0f} 秒后进入下一轮...\n")
        time.sleep(sleep_time)

    if mqtt_client:
        mqtt_client.loop_stop()
        mqtt_client.disconnect()


if __name__ == "__main__":
    main()
