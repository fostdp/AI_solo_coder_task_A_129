# 古代铜鼓铸造工艺仿真与声学特性分析系统 v2.0.0

## 项目简介

面向西南少数民族铜鼓复原研究的全栈工程化系统。集成铸造工艺仿真、声学特性分析、实时告警推送、三维可视化展示四大核心能力，支持多面铜鼓并行监测与数据分析。

---

## 系统架构

```
                                                                  ┌─────────────────────────┐
                                                                  │   Prometheus (9090)     │
                                                                  │  Metrics Aggregation    │
                                                                  └───────────┬─────────────┘
                                                                              │
                                                                              │ scrape
                                                                              ▼
┌──────────────────┐   HTTP/MQTT   ┌──────────────────┐   ClickHouse   ┌──────────────────┐
│  Copper Drum     │ ────────────► │  Rust Backend    │ ─────────────► │  ClickHouse       │
│  Simulator       │   Sensor Data │  (8080)          │   Raw +        │  (8123/9000)      │
│  (Python)        │                │  Microservices   │   Aggregated   │  3-layer          │
│                  │                │  - dtu_receiver  │   Data         │  Downsampling     │
│  Configurable:   │                │  - casting_sim   │                │  + TTL Retention  │
│  - Alloy Formu-  │                │  - acoustic_anlz │                └──────────────────┘
│    las (5+)      │                │  - alarm_mqtt    │
│  - Wall Thick-   │                │  Tokio MPSC      │         ┌──────────────────────────┐
│    ness (5+)     │                │  Channels        │         │  Frontend (Nginx :80)     │
│  - 6 Drum Faces  │                │                  │         │  - Gzip Compression       │
│                  │                │  /metrics        │         │  - 3D Visualization       │
│                  │                │  /api/health     │         │  - Vibration Modes        │
└──────────────────┘                │  /api/alarms/stream │     │  - Sound Field Cloud      │
          ▲                         │                  │         │  - Static Cache (7d)      │
          │                         └─────────┬────────┘         └─────────────┬────────────┘
          │                                   │                                │
          │                                   │                                │
          │ MQTT Publish                      │ MQTT Subscribe                 │ HTTP
          ▼                                   ▼                                ▼
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│                                    MQTT Broker (Mosquitto)                               │
│                                    Port: 1883 (MQTT) / 9001 (WebSocket)                  │
│                                    Topics: bronze/drum/+/sensor, bronze/drum/+/alarm     │
└──────────────────────────────────────────────────────────────────────────────────────────┘
                                          ▲
                                          │
                                          │ metrics
                                          ▼
                                  ┌──────────────────┐
                                  │  MQTT Exporter   │
                                  │  (9234)          │
                                  └──────────────────┘

 Service Dependencies (Health Check Based):
 ┌────────────┐     ┌────────────┐     ┌────────────┐
 │ clickhouse │────►│  backend   │────►│ simulator  │
 └────────────┘     └────────────┘     └────────────┘
          ▲                ▲                ▲
          │                │                │
 ┌────────────┐     ┌────────────┐     ┌────────────┐
 │    mqtt    │────►│ mqtt-exp   │     │  frontend  │
 └────────────┘     └────────────┘     └────────────┘
          ▲                ▲
          │                │
          └────────────────┴───────────────┐
                                           │
                                        ┌──────┐
                                        │ prom │
                                        └──────┘
```

---

## 技术栈

| 层级 | 技术选型 | 版本 | 说明 |
|------|----------|------|------|
| **后端** | Rust + Tokio + Axum | 1.77+ | 异步微服务架构，Tokio MPSC 通道通信 |
| **数据库** | ClickHouse | 24.3 | 列式时序数据库，MergeTree + SummingMergeTree |
| **消息队列** | Eclipse Mosquitto | 2.0.18 | MQTT 3.1.1，双监听器 |
| **前端** | Three.js + Canvas | - | 铜鼓三维模型、振动模态动画、声场云图 |
| **可观测性** | Prometheus + Tracing | 2.51 | metrics 采集 + tracing 结构化日志 |
| **容器化** | Docker + Docker Compose | 24.0+ | 多阶段构建，Alpine 最小镜像 |
| **模拟器** | Python | 3.11 | 可配置合金配方 + 壁厚轮廓数据生成 |

---

## 核心特性

### 1. Rust 后端 - 微服务通道架构
- **dtu_receiver**: 传感器数据采集与校验，异常数据过滤
- **casting_simulator**: 凝固仿真 + 缩孔预测，支持网格细化
- **acoustic_analyzer**: 有限元声学计算，弧长法求解非线性振动
- **alarm_mqtt**: 告警评估 + MQTT/ WebSocket 双协议推送
- **Prometheus 指标**: `/metrics` 端点暴露 8 类指标
- **Tracing 日志**: 结构化日志，支持 `RUST_LOG` 环境变量过滤

### 2. ClickHouse - 三层降采样 + TTL
```
Raw Data (30d) → Hourly Agg (6m) → Daily Agg (2y)
    │                 │                  │
    └─ sensor_readings  └─ mv_sensor_hourly  └─ mv_sensor_daily
    └─ TTL: timestamp + INTERVAL 30 DAY
    └─ ReplacingMergeTree 去重
    └─ SummingMergeTree 自动聚合（告警统计 5 年保留）
```

### 3. 前端 Nginx - Gzip 压缩优化
- **18 种 MIME 类型压缩**: text/html, text/css, application/javascript, image/svg+xml 等
- **压缩级别**: 6 (平衡压缩率与 CPU)
- **静态资源缓存**: 7 天强缓存 (`Cache-Control: public, max-age=604800, immutable`)
- **安全头**: X-Frame-Options, X-XSS-Protection, X-Content-Type-Options
- **API 反向代理**: `/api/` → `backend:8080`，WebSocket 告警流支持

### 4. 铜鼓工艺模拟器 - 可配置数据生成
- **5 种合金配方**: 壮族标准、苗族高锡、侗族低铅、彝族早期、白族定制
- **5 种壁厚轮廓**: uniform, center_thick, edge_thick, wavy, defect_prone
- **6 面铜鼓档案**: 每面独立配置合金与壁厚
- **异常注入**: 支持 alloy/thickness/frequency 异常模拟

---

## 部署步骤

### 前置要求
- Docker ≥ 24.0
- Docker Compose ≥ 2.20
- 可用端口: 80, 8080, 8123, 9000, 1883, 9001, 9090, 9234

### 快速部署

```bash
# 1. 克隆项目
git clone <repository-url>
cd AI_solo_coder_task_A_129

# 2. 检查环境变量配置
cat .env

# 3. 构建并启动所有服务 (后台运行)
docker compose up -d --build

# 4. 查看服务状态
docker compose ps

# 5. 查看服务日志
docker compose logs -f backend    # 后端日志
docker compose logs -f simulator  # 模拟器日志
docker compose logs -f clickhouse # 数据库日志

# 6. 停止服务
docker compose down

# 7. 停止并清除数据卷（慎用）
docker compose down -v
```

### 服务访问地址

| 服务 | 地址 | 说明 |
|------|------|------|
| **前端** | http://localhost/ | 铜鼓三维可视化界面 |
| **后端 API** | http://localhost:8080/api/health | 健康检查 |
| **Prometheus 指标** | http://localhost:8080/metrics | Rust 指标端点 |
| **Prometheus UI** | http://localhost:9090 | 指标查询界面 |
| **ClickHouse** | http://localhost:8123 | HTTP 接口 |
| **MQTT Broker** | mqtt://localhost:1883 | MQTT 协议 |
| **MQTT WebSocket** | ws://localhost:9001 | WebSocket 协议 |
| **MQTT Exporter** | http://localhost:9234/metrics | MQTT 指标 |

### 健康检查验证

```bash
# 验证后端健康
curl http://localhost:8080/api/health

# 验证 Prometheus 指标
curl http://localhost:8080/metrics | head -20

# 验证 ClickHouse
curl http://localhost:8123/ping

# 验证前端 Gzip 压缩
curl -I -H "Accept-Encoding: gzip" http://localhost/ | grep -i content-encoding
```

---

## 模拟器用法

### 配置文件结构

```json
// simulator/simulator_config.json
{
  "alloys": {
    "zhuang_standard": {
      "name": "壮族标准青铜",
      "copper_pct": 77.8, "tin_pct": 18.3, "lead_pct": 3.2
    },
    "miao_high_tin": { "...": "..." }
  },
  "thickness_profiles": {
    "uniform": { "...": "..." },
    "center_thick": { "...": "..." }
  },
  "drums": [
    {
      "drum_id": "drum-zhuang-001",
      "name": "灵山型铜鼓",
      "face": "鼓面",
      "base_alloy": "zhuang_standard",
      "base_thickness": "center_thick"
    }
  ]
}
```

### 命令行参数

```bash
cd simulator

# 查看帮助
python drum_simulator.py --help

# 列出所有可用合金配方
python drum_simulator.py --list-alloys

# 列出所有可用壁厚轮廓
python drum_simulator.py --list-thickness

# 单次运行（默认 3600s 间隔）
python drum_simulator.py --interval 60

# 覆盖所有鼓的合金配方
python drum_simulator.py --alloy miao_high_tin --interval 60

# 覆盖所有鼓的壁厚轮廓
python drum_simulator.py --thickness wavy --interval 60

# 同时覆盖合金和壁厚
python drum_simulator.py --alloy dong_low_lead --thickness edge_thick --interval 60

# 固定随机种子复现场景
python drum_simulator.py --seed 42 --interval 60

# 注入异常（类型: alloy/thickness/frequency/all）
python drum_simulator.py --anomaly frequency --interval 60

# 仅上报 MQTT
python drum_simulator.py --transport mqtt --interval 60

# 仅上报 HTTP
python drum_simulator.py --transport http --interval 60

# Docker 方式运行（使用 docker-compose 内置）
docker compose run --rm simulator --help
docker compose run --rm simulator --alloy bai_custom_alloy --interval 30
```

### Docker 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `API_BASE` | http://backend:8080 | 后端 API 地址 |
| `MQTT_HOST` | mqtt | MQTT Broker 主机名 |
| `MQTT_PORT` | 1883 | MQTT Broker 端口 |

### 合金配方库（5 种）

| 配方 ID | 名称 | Cu% | Sn% | Pb% | 适用民族 |
|---------|------|-----|-----|-----|----------|
| `zhuang_standard` | 壮族标准青铜 | 77.8 | 18.3 | 3.2 | 壮族 |
| `miao_high_tin` | 苗族高锡青铜 | 72.5 | 22.1 | 4.8 | 苗族 |
| `dong_low_lead` | 侗族低铅青铜 | 80.2 | 15.6 | 3.0 | 侗族 |
| `yi_early_bronze` | 彝族早期青铜 | 82.1 | 14.2 | 2.8 | 彝族 |
| `bai_custom_alloy` | 白族定制合金 | 75.0 | 19.5 | 4.5 | 白族 |

### 壁厚轮廓库（5 种）

| 轮廓 ID | 说明 | 算法特点 |
|---------|------|----------|
| `uniform` | 均匀壁厚 | 恒定厚度 + 小噪声 |
| `center_thick` | 中心厚边缘薄 | 中心 8mm → 边缘 4mm 线性递减 |
| `edge_thick` | 边缘厚中心薄 | 边缘 7mm → 中心 3mm 线性递减 |
| `wavy` | 波浪形分布 | 正弦波叠加，±1.5mm 振幅 |
| `defect_prone` | 易缺陷型 | 局部减薄区 + 随机缺陷点 |

---

## Prometheus 指标说明

### 核心指标

| 指标名 | 类型 | 说明 |
|--------|------|------|
| `bronze_http_requests_total` | Counter | HTTP 请求总数，labels: method, path, status |
| `bronze_http_request_duration_seconds` | Histogram | HTTP 请求耗时 |
| `bronze_sensor_readings_total` | Counter | 传感器读数总数，labels: drum_id, valid |
| `bronze_casting_simulations_total` | Counter | 铸造仿真总数，labels: drum_id, quality_ok |
| `bronze_acoustic_analyses_total` | Counter | 声学分析总数，labels: drum_id, pass |
| `bronze_alarms_total` | Counter | 告警触发总数，labels: severity, type |
| `bronze_alarms_pending` | Gauge | 待推送告警数 |
| `bronze_active_sessions` | Gauge | 活跃铜鼓会话数 |
| `bronze_process_uptime_seconds` | Gauge | 进程运行时间（秒） |
| `bronze_build_info` | Gauge | 构建信息，labels: version, arch |

### 查询示例

```promql
// 每分钟 HTTP 请求率（按路径）
rate(bronze_http_requests_total[5m])

// 传感器读数总数（按铜鼓）
sum by (drum_id) (increase(bronze_sensor_readings_total[1h]))

// 铸造仿真质量合格率
sum(rate(bronze_casting_simulations_total{quality_ok="true"}[1h]))
/
sum(rate(bronze_casting_simulations_total[1h]))

// 告警趋势
sum by (severity) (increase(bronze_alarms_total[24h]))
```

---

## 项目结构

```
AI_solo_coder_task_A_129/
├── backend/                    # Rust 后端
│   ├── src/
│   │   ├── main.rs            # 入口，服务编排
│   │   ├── metrics.rs         # Prometheus 指标模块
│   │   ├── api.rs             # REST API + 指标埋点
│   │   ├── dtu_receiver.rs    # DTU 数据接收模块
│   │   ├── casting_simulator.rs  # 铸造仿真模块
│   │   ├── acoustic_analyzer.rs   # 声学分析模块
│   │   ├── alarm_mqtt.rs      # 告警 MQTT 模块
│   │   ├── clickhouse_client.rs  # ClickHouse 客户端
│   │   ├── mqtt_client.rs     # MQTT 客户端
│   │   ├── config.rs          # 配置加载
│   │   └── models.rs          # 数据模型
│   ├── config/                # JSON 配置文件
│   ├── Cargo.toml
│   └── Dockerfile             # 多阶段构建
├── frontend/                   # 前端
│   ├── js/
│   │   ├── bronze_drum_3d.js  # 三维渲染模块
│   │   ├── acoustic_panel.js  # 声学面板模块
│   │   └── main.js            # 入口
│   ├── index.html
│   ├── nginx.conf             # Gzip + 缓存配置
│   └── Dockerfile
├── clickhouse/                 # ClickHouse 配置
│   └── init_full.sql          # 表结构 + 降采样 + TTL
├── mqtt/                       # MQTT Broker 配置
│   └── mosquitto.conf
├── prometheus/                 # Prometheus 配置
│   └── prometheus.yml         # 采集目标配置
├── simulator/                  # 铜鼓工艺模拟器
│   ├── drum_simulator.py      # 模拟器主程序
│   ├── simulator_config.json  # 合金+壁厚配置
│   ├── requirements.txt
│   └── Dockerfile
├── .env                        # 环境变量
├── .dockerignore               # Docker 忽略文件
├── docker-compose.yml          # 服务编排
└── README.md                   # 本文档
```

---

## 常见问题

### Q: ClickHouse 初始化失败？
A: 检查 `clickhouse/init_full.sql` 是否挂载正确，首次启动需要约 30 秒初始化。

### Q: Rust 后端连接 MQTT 失败？
A: 检查 docker-compose 服务启动顺序，backend 依赖 mqtt 的 healthcheck 通过后才会启动。

### Q: 模拟器不发送数据？
A: 检查 `API_BASE` 和 `MQTT_HOST` 环境变量，使用 `docker compose logs simulator` 查看日志。

### Q: 前端无法加载三维模型？
A: 检查浏览器控制台，确认 Nginx 是否正确启用 Gzip，静态资源是否返回 200。

### Q: Prometheus 无数据？
A: 访问 http://localhost:9090/targets 检查各目标状态，确认防火墙未阻止 8080 端口。

---

## License

MIT
