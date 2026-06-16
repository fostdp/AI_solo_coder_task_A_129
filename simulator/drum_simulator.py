#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
古代铜鼓铸造工艺仿真与声学特性分析系统
铜鼓工艺模拟器 v2.0.0
========================================

功能：
1. 通过 JSON 配置文件管理多种合金配方和壁厚分布
2. 命令行参数可覆盖合金配方和壁厚轮廓
3. 每小时生成模拟传感器数据（合金成分、壁厚分布、敲击音频谱）
4. 通过 HTTP POST 发送到后端 /api/sensor/readings
5. 支持 MQTT 发布到 bronze-drum/sensors/{drum_id}
6. 支持引入随机扰动以模拟真实测量噪声
7. 可手动触发铸造缺陷事件和音准漂移事件

合金配方（可配置）：
  --alloy zhuang_standard   壮族标准锡铅青铜 (77.8Cu/18.3Sn/3.2Pb)
  --alloy miao_high_tin     苗族高锡青铜 (75.5Cu/20.1Sn/3.8Pb)
  --alloy dong_low_lead     侗族低铅青铜 (79.5Cu/17.2Sn/2.5Pb)
  --alloy yi_early_bronze   彝族早期青铜 (82.0Cu/15.5Sn/1.8Pb)
  --alloy bai_custom_alloy  白族定制配方 (73.0Cu/12Sn/13Pb)

壁厚轮廓（可配置）：
  --thickness uniform       均匀壁厚 6mm ±5%
  --thickness center_thick  中心厚(8mm) 边缘薄(4mm)
  --thickness edge_thick    边缘厚(8mm) 中心薄(4mm)
  --thickness wavy          波浪形壁厚 4波
  --thickness defect_prone  易缺陷分布 25%区域薄至60%
"""

import argparse
import json
import math
import os
import random
import sys
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import requests

try:
    import paho.mqtt.client as mqtt
    HAS_MQTT = True
except ImportError:
    HAS_MQTT = False

# ============================================================
# 默认配置
# ============================================================
API_BASE = "http://backend:8080"
MQTT_HOST = "mqtt"
MQTT_PORT = 1883
MQTT_SENSOR_TOPIC = "bronze-drum/sensors"
CONFIG_FILE = Path(__file__).parent / "simulator_config.json"

ZONES = [
    "鼓心/太阳纹区", "主晕圈/羽人纹区", "主晕圈/羽人纹区",
    "鼓面外圈/立蛙区", "鼓面外圈/立蛙区", "鼓腰/胴部",
    "鼓腰/胴部", "鼓足/底部边缘", "鼓足/底部边缘",
    "耳部/纹饰区", "鼓心/太阳纹区", "鼓面外圈/立蛙区",
]

# ============================================================
# 配置加载
# ============================================================
class SimulatorConfig:
    def __init__(self, config_path: Path):
        self.config_path = config_path
        self.alloy_recipes: Dict[str, Dict] = {}
        self.thickness_profiles: Dict[str, Dict] = {}
        self.drums: List[Dict] = []
        self._load()

    def _load(self):
        with open(self.config_path, 'r', encoding='utf-8') as f:
            data = json.load(f)
        self.alloy_recipes = data.get('alloy_recipes', {})
        self.thickness_profiles = data.get('thickness_profiles', {})
        self.drums = data.get('drums', [])
        print(f"📋 加载配置: {len(self.alloy_recipes)} 种合金配方, {len(self.thickness_profiles)} 种壁厚轮廓, {len(self.drums)} 面铜鼓")

    def get_alloy(self, name: str) -> Dict:
        return self.alloy_recipes.get(name, self.alloy_recipes.get('zhuang_standard', {}))

    def get_thickness(self, name: str) -> Dict:
        return self.thickness_profiles.get(name, self.thickness_profiles.get('uniform', {}))

    def list_alloys(self) -> List[str]:
        return list(self.alloy_recipes.keys())

    def list_thickness_profiles(self) -> List[str]:
        return list(self.thickness_profiles.keys())

# ============================================================
# 数据生成
# ============================================================
class DataGenerator:
    @staticmethod
    def gaussian(mean: float, std: float) -> float:
        return random.gauss(mean, std)

    @staticmethod
    def generate_alloy(recipe: Dict, drift_event: bool = False) -> Dict:
        """根据合金配方生成成分，含X射线荧光光谱仪的典型误差"""
        noise = 0.12
        result = {
            "copper_pct": round(DataGenerator.gaussian(recipe["copper_pct"], noise), 2),
            "tin_pct": round(DataGenerator.gaussian(recipe["tin_pct"], noise * 0.8), 2),
            "lead_pct": round(DataGenerator.gaussian(recipe["lead_pct"], noise * 0.5), 2),
            "zinc_pct": round(abs(DataGenerator.gaussian(recipe.get("zinc_pct", 0.4), noise * 0.3)), 2),
            "other_impurities_pct": round(abs(DataGenerator.gaussian(recipe.get("other_pct", 0.3), noise * 0.2)), 2),
        }

        if drift_event:
            result["tin_pct"] = round(recipe["tin_pct"] - random.uniform(2.0, 4.5), 2)
            result["copper_pct"] = round(recipe["copper_pct"] + random.uniform(1.5, 3.5), 2)

        total = sum(result.values())
        if abs(total - 100.0) > 0.5:
            scale = 100.0 / total
            for k in result:
                result[k] = round(result[k] * scale, 2)

        return result

    @staticmethod
    def generate_wall_thickness(profile: Dict, diameter_cm: float,
                                 anomaly: bool = False) -> List[Dict]:
        """根据壁厚轮廓生成12个测点，模拟鼓形几何特征与测量误差"""
        points = []
        profile_name = profile.get("name", "unknown")

        for i, zone in enumerate(ZONES):
            angle = (i / len(ZONES)) * 2 * math.pi
            x_frac = 0.5 + 0.4 * math.cos(angle) * random.uniform(0.85, 1.15)
            y_frac = 0.5 + 0.4 * math.sin(angle) * random.uniform(0.85, 1.15)

            r = math.sqrt((x_frac - 0.5) ** 2 + (y_frac - 0.5) ** 2) * 2

            base_mm = profile.get("base_mm", 6.0)
            variation_pct = profile.get("variation_pct", 5.0) / 100.0

            if profile_name == "center_thick":
                center = profile.get("center_mm", 8.0)
                edge = profile.get("edge_mm", 4.0)
                thickness = center - r * (center - edge)
            elif profile_name == "edge_thick":
                center = profile.get("center_mm", 4.0)
                edge = profile.get("edge_mm", 8.0)
                thickness = center + r * (edge - center)
            elif profile_name == "wavy":
                amplitude = profile.get("amplitude_mm", 1.5)
                wave_count = profile.get("wave_count", 4)
                wave = amplitude * math.sin(r * math.pi * wave_count)
                thickness = base_mm + wave
            elif profile_name == "defect_prone":
                thin_zone_pct = profile.get("thin_zone_pct", 25) / 100.0
                thin_factor = profile.get("thin_factor", 0.6)
                angle_region = (i / len(ZONES)) < thin_zone_pct
                thickness = base_mm * (thin_factor if angle_region else 1.0)
            else:  # uniform
                geometry_factor = 1.0 + 0.15 * math.sin(r * math.pi * 2) - 0.1 * abs(r - 0.5)
                thickness = base_mm * geometry_factor

            thickness *= random.uniform(1.0 - variation_pct, 1.0 + variation_pct)

            if anomaly and random.random() < 0.25:
                thickness *= random.uniform(0.55, 0.75)

            points.append({
                "zone": zone,
                "x_frac": round(x_frac, 4),
                "y_frac": round(y_frac, 4),
                "thickness_mm": round(thickness, 3),
            })

        return points

    @staticmethod
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
                "amplitude": round(max(amplitude, -80.0), 2),
            })

        return spectrum

    @staticmethod
    def build_sensor_reading(drum_cfg: Dict, alloy_recipe: Dict, thickness_profile: Dict,
                              failure_event: bool = False) -> Dict:
        """组装一次完整的传感器读数"""
        now = datetime.now(timezone.utc)
        reading_id = str(uuid.uuid4())

        anomaly_type = None
        if failure_event:
            anomaly_type = random.choice(["alloy", "thickness", "frequency", "all"])

        alloy = DataGenerator.generate_alloy(
            alloy_recipe,
            drift_event=(anomaly_type in ("alloy", "all")),
        )

        wall = DataGenerator.generate_wall_thickness(
            thickness_profile,
            drum_cfg.get("diameter_cm", 50.0),
            anomaly=(anomaly_type in ("thickness", "all")),
        )

        spectrum = DataGenerator.generate_tap_spectrum(
            drum_cfg.get("resonance_freqs_hz", [523.25, 659.25, 783.99]),
            freq_drift=(anomaly_type in ("frequency", "all")),
        )

        return {
            "reading_id": reading_id,
            "drum_id": drum_cfg["drum_id"],
            "timestamp": now.isoformat(),
            "alloy": alloy,
            "wall_thickness": wall,
            "tap_spectrum": spectrum,
            "temperature_c": round(DataGenerator.gaussian(24.0, 1.8), 1),
            "ambient_humidity_pct": round(DataGenerator.gaussian(55.0, 8.0), 1),
            "sensor_ids": [
                f"XRF-{random.randint(100, 999)}",
                f"UT-{random.randint(1000, 9999)}",
                f"MIC-{random.randint(10000, 99999)}",
                f"ENV-{random.randint(1000, 9999)}",
            ],
            "_metadata": {
                "alloy_recipe": alloy_recipe.get("name", "unknown"),
                "thickness_profile": thickness_profile.get("name", "unknown"),
                "anomaly_type": anomaly_type,
            },
        }

# ============================================================
# 通信层
# ============================================================
class DataSender:
    @staticmethod
    def send_http(reading: Dict, api_base: str, timeout: float = 30.0) -> Tuple[bool, Optional[str]]:
        """通过HTTP API上报读数"""
        url = f"{api_base}/api/sensor/readings"
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

    @staticmethod
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

    @staticmethod
    def ensure_drums_registered(drums: List[Dict], api_base: str):
        """如果后端没有鼓档案，则先初始化"""
        try:
            resp = requests.get(f"{api_base}/api/drums", timeout=10)
            if resp.status_code == 200:
                data = resp.json()
                existing = {d["drum_id"] for d in (data.get("data") or [])}
                for drum in drums:
                    if drum["drum_id"] not in existing:
                        print(f"注册铜鼓档案: {drum['name']} ({drum['drum_id']})")
                        body = {
                            "drum_id": drum["drum_id"],
                            "name": drum["name"],
                            "ethnic_group": drum.get("ethnic_group", "未知"),
                            "origin_region": f"模拟-{drum.get('ethnic_group', '未知')}地区",
                            "estimated_era": "模拟数据-现代复原",
                            "diameter_cm": drum.get("diameter_cm", 50.0),
                            "height_cm": drum.get("height_cm", 30.0),
                            "mass_kg": round(drum.get("diameter_cm", 50.0) * 0.55, 1),
                            "notes": "传感器模拟器自动创建的模拟铜鼓档案",
                        }
                        requests.post(f"{api_base}/api/drums", json=body, timeout=10)
        except Exception as e:
            print(f"检查/注册铜鼓档案失败 (可能后端未启动): {e}", file=sys.stderr)

# ============================================================
# 主程序
# ============================================================
def main():
    parser = argparse.ArgumentParser(description="铜鼓工艺模拟器 v2.0.0 - 支持可配置合金和壁厚",
                                      formatter_class=argparse.RawDescriptionHelpFormatter,
                                      epilog=__doc__)
    parser.add_argument("--config", type=str, default=str(CONFIG_FILE),
                        help=f"配置文件路径 (默认 {CONFIG_FILE})")
    parser.add_argument("--interval", type=int, default=3600,
                        help="上报间隔秒数（默认3600=1小时）")
    parser.add_argument("--drum", type=str, default=None,
                        help="只模拟指定drum_id")
    parser.add_argument("--alloy", type=str, default=None,
                        help="覆盖所有鼓的合金配方名（见配置文件alloy_recipes）")
    parser.add_argument("--thickness", type=str, default=None,
                        help="覆盖所有鼓的壁厚轮廓名（见配置文件thickness_profiles）")
    parser.add_argument("--inject-failure", action="store_true",
                        help="单次注入异常事件后退出")
    parser.add_argument("--run-once", action="store_true",
                        help="只发送一轮读数后退出")
    parser.add_argument("--mqtt", action="store_true",
                        help="同时启用MQTT发布")
    parser.add_argument("--api", type=str, default=os.environ.get("API_BASE", API_BASE),
                        help=f"后端API地址 (默认 {API_BASE})")
    parser.add_argument("--list-alloys", action="store_true",
                        help="列出可用的合金配方并退出")
    parser.add_argument("--list-thickness", action="store_true",
                        help="列出可用的壁厚轮廓并退出")
    parser.add_argument("--verbose", action="store_true",
                        help="显示详细日志")
    parser.add_argument("--seed", type=int, default=None,
                        help="随机数种子（复现特定场景）")
    args = parser.parse_args()

    if args.seed is not None:
        random.seed(args.seed)
        print(f"🎲 随机数种子已设为: {args.seed}")

    # 加载配置
    cfg = SimulatorConfig(Path(args.config))

    # 列表展示模式
    if args.list_alloys:
        print("\n📚 可用的合金配方:")
        for name, recipe in cfg.alloy_recipes.items():
            print(f"  {name:<20} {recipe.get('name','')}")
            print(f"    Cu: {recipe['copper_pct']:>5.1f}% | Sn: {recipe['tin_pct']:>5.1f}% | Pb: {recipe['lead_pct']:>5.1f}%")
        return
    if args.list_thickness:
        print("\n📐 可用的壁厚轮廓:")
        for name, profile in cfg.thickness_profiles.items():
            print(f"  {name:<20} {profile.get('name','')}")
        return

    API_BASE = args.api

    # 选择铜鼓
    drums = [d for d in cfg.drums if args.drum is None or d["drum_id"] == args.drum]
    if not drums:
        print(f"未找到匹配的铜鼓: {args.drum}")
        sys.exit(1)

    # 应用合金/壁厚覆盖
    if args.alloy:
        if args.alloy not in cfg.alloy_recipes:
            print(f"❌ 未找到合金配方: {args.alloy}")
            print(f"   可用配方: {', '.join(cfg.list_alloys())}")
            sys.exit(1)
        for d in drums:
            d["base_alloy"] = args.alloy
        print(f"🔧 合金配方已覆盖为: {args.alloy}")

    if args.thickness:
        if args.thickness not in cfg.thickness_profiles:
            print(f"❌ 未找到壁厚轮廓: {args.thickness}")
            print(f"   可用轮廓: {', '.join(cfg.list_thickness_profiles())}")
            sys.exit(1)
        for d in drums:
            d["base_thickness"] = args.thickness
        print(f"🔧 壁厚轮廓已覆盖为: {args.thickness}")

    # 注册铜鼓档案
    DataSender.ensure_drums_registered(drums, API_BASE)

    # MQTT 初始化
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

    # 启动信息
    print(f"\n🛢️  铜鼓工艺模拟器 v2.0.0")
    print(f"   监控铜鼓: {len(drums)} 面")
    for d in drums:
        alloy = cfg.get_alloy(d["base_alloy"]).get("name", "unknown")
        thick = cfg.get_thickness(d["base_thickness"]).get("name", "unknown")
        print(f"     - {d['name']} ({d['drum_id']})")
        print(f"       合金: {alloy} | 壁厚: {thick}")
    print(f"   上报间隔: {args.interval} 秒 ({args.interval/60:.1f} 分钟)")
    print(f"   API地址:  {API_BASE}")
    print(f"   MQTT:     {'启用' if mqtt_client else '禁用'}")
    print(f"   模式:     {'异常注入' if args.inject_failure else ('单次运行' if args.run_once else '循环运行')}")
    print("=" * 70 + "\n")

    # 主循环
    while True:
        round_start = time.time()
        print(f"[{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}] 📡 开始新的采集轮次")

        for idx, drum in enumerate(drums, 1):
            should_fail = args.inject_failure and idx == len(drums)
            alloy_recipe = cfg.get_alloy(drum["base_alloy"])
            thickness_profile = cfg.get_thickness(drum["base_thickness"])

            reading = DataGenerator.build_sensor_reading(drum, alloy_recipe, thickness_profile,
                                                           failure_event=should_fail)

            alloy = reading["alloy"]
            avg_t = sum(p["thickness_mm"] for p in reading["wall_thickness"]) / len(reading["wall_thickness"])
            meta = reading.get("_metadata", {})

            print(f"  [{idx}/{len(drums)}] {drum['name']} ({drum['drum_id']})")
            if args.verbose:
                print(f"    合金: Cu={alloy['copper_pct']}%, Sn={alloy['tin_pct']}%, Pb={alloy['lead_pct']}%")
                print(f"    平均壁厚: {avg_t:.2f}mm, 谱点: {len(reading['tap_spectrum'])}")
                print(f"    温湿度: {reading['temperature_c']}℃ / {reading['ambient_humidity_pct']}%")
                print(f"    配方: {meta.get('alloy_recipe')} | 轮廓: {meta.get('thickness_profile')}")
                if meta.get("anomaly_type"):
                    print(f"    ⚠️  异常类型: {meta['anomaly_type']}")

            ok, err = DataSender.send_http(reading, API_BASE)
            if ok:
                print(f"    ✅ HTTP上报成功")
            else:
                print(f"    ❌ HTTP上报失败: {err}", file=sys.stderr)

            if mqtt_client:
                if DataSender.send_mqtt(mqtt_client, reading, drum["drum_id"]):
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
