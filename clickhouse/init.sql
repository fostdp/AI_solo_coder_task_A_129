-- ============================================================
-- 古代铜鼓铸造工艺仿真与声学特性分析系统
-- ClickHouse 数据库初始化脚本
-- ============================================================

CREATE DATABASE IF NOT EXISTS bronze_drum
    ENGINE = Atomic
    COMMENT '古代铜鼓系统数据库';

USE bronze_drum;

-- ============================================================
-- 铜鼓主表：存储铜鼓基本信息
-- ============================================================
CREATE TABLE IF NOT EXISTS drums (
    drum_id         String,
    name            String              COMMENT '铜鼓名称/编号',
    ethnic_group    String              COMMENT '所属少数民族',
    origin_region   String              COMMENT '原出土/流传地区',
    estimated_era   String              COMMENT '估计年代',
    diameter_cm     Float64             COMMENT '直径(厘米)',
    height_cm       Float64             COMMENT '高度(厘米)',
    mass_kg         Float64             COMMENT '重量(千克)',
    created_at      DateTime64(9, 'UTC') DEFAULT now64(9),
    notes           Nullable(String)    COMMENT '备注信息'
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(created_at)
ORDER BY (drum_id, created_at)
PRIMARY KEY drum_id
COMMENT '铜鼓档案表';

-- ============================================================
-- 传感器读数表：每小时上报一次
-- ============================================================
CREATE TABLE IF NOT EXISTS sensor_readings (
    reading_id      String,
    drum_id         String,
    timestamp       DateTime64(9, 'UTC') DEFAULT now64(9),
    copper_pct      Float64             COMMENT '铜含量百分比',
    tin_pct         Float64             COMMENT '锡含量百分比',
    lead_pct        Float64             COMMENT '铅含量百分比',
    zinc_pct        Float64             COMMENT '锌含量百分比',
    other_pct       Float64             COMMENT '其他杂质百分比',
    wall_thickness  String              COMMENT '壁厚分布JSON数组',
    tap_spectrum    String              COMMENT '敲击音频谱JSON数组',
    temperature_c   Float64 DEFAULT 25.0 COMMENT '环境温度(℃)',
    humidity_pct    Float64 DEFAULT 50.0 COMMENT '环境湿度(%)',
    sensor_ids      String              COMMENT '参与采集的传感器ID数组'
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (drum_id, timestamp, reading_id)
PRIMARY KEY (drum_id, timestamp)
TTL timestamp + INTERVAL 5 YEAR
COMMENT '传感器采集数据表(每小时)';

CREATE INDEX IF NOT EXISTS idx_sensor_drum ON sensor_readings (drum_id) TYPE minmax GRANULARITY 1;

-- ============================================================
-- 壁厚历史明细表
-- ============================================================
CREATE TABLE IF NOT EXISTS wall_thickness_history (
    drum_id         String,
    timestamp       DateTime64(9, 'UTC'),
    zone            String              COMMENT '区域名称',
    x_frac          Float64             COMMENT '相对X坐标(0-1)',
    y_frac          Float64             COMMENT '相对Y坐标(0-1)',
    thickness_mm    Float64             COMMENT '壁厚(毫米)'
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (drum_id, zone, timestamp)
COMMENT '壁厚测点历史';

-- ============================================================
-- 铸造仿真结果表
-- ============================================================
CREATE TABLE IF NOT EXISTS casting_simulations (
    sim_id          String,
    drum_id         String,
    created_at      DateTime64(9, 'UTC') DEFAULT now64(9),
    copper_pct      Float64,
    tin_pct         Float64,
    lead_pct        Float64,
    zinc_pct        Float64,
    other_pct       Float64,
    pour_temp       Float64             COMMENT '浇注温度(℃)',
    mold_temp       Float64             COMMENT '铸型温度(℃)',
    cooling_time    Float64             COMMENT '冷却时间(秒)',
    solidus_temp    Float64             COMMENT '固相线温度(℃)',
    liquidus_temp   Float64             COMMENT '液相线温度(℃)',
    shrinkage_map   String              COMMENT '缩孔风险分布图(x,y,risk)',
    cooling_rate_map String             COMMENT '冷却速率分布图(x,y,rate)',
    defects         String              COMMENT '预测缺陷列表JSON',
    quality_score   Float64             COMMENT '品质综合评分(0-1)',
    overall_risk    LowCardinality(String) COMMENT '整体风险等级'
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(created_at)
ORDER BY (drum_id, created_at, sim_id)
PRIMARY KEY (drum_id, created_at)
COMMENT '铸造工艺仿真结果';

-- ============================================================
-- 声学分析结果表
-- ============================================================
CREATE TABLE IF NOT EXISTS acoustic_analyses (
    analysis_id     String,
    drum_id         String,
    created_at      DateTime64(9, 'UTC') DEFAULT now64(9),
    youngs_modulus  Float64             COMMENT '杨氏模量(Pa)',
    poissons_ratio  Float64             COMMENT '泊松比',
    density         Float64             COMMENT '合金密度(kg/m³)',
    sound_speed     Float64             COMMENT '空气中声速(m/s)',
    air_density     Float64             COMMENT '空气密度(kg/m³)',
    vibration_modes String              COMMENT '振动模态列表JSON',
    radiated_power  Float64             COMMENT '辐射声功率(W)',
    resonance_freqs String              COMMENT '共振频率列表(Hz)',
    sound_field     String              COMMENT '声场云图点(x,y,z,p,spl)',
    sound_quality   Float64             COMMENT '声学品质评分(0-1)'
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(created_at)
ORDER BY (drum_id, created_at, analysis_id)
PRIMARY KEY (drum_id, created_at)
COMMENT '声学特性有限元分析结果';

-- ============================================================
-- 告警表：铸造缺陷与音准偏差触发的告警
-- ============================================================
CREATE TABLE IF NOT EXISTS alarms (
    alarm_id        String,
    drum_id         String,
    timestamp       DateTime64(9, 'UTC') DEFAULT now64(9),
    alarm_type      LowCardinality(String) COMMENT '告警类型',
    severity        LowCardinality(String) COMMENT '严重等级(Info/Warning/Critical/Fatal)',
    message         String              COMMENT '告警消息',
    measured_value  Float64             COMMENT '实际测量值',
    threshold_value Float64             COMMENT '触发阈值',
    metadata        String              COMMENT '附加数据(JSON)',
    acknowledged    UInt8 DEFAULT 0     COMMENT '是否已确认'
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (drum_id, timestamp, alarm_id)
PRIMARY KEY (drum_id, timestamp)
TTL timestamp + INTERVAL 2 YEAR
COMMENT '告警历史表';

CREATE INDEX IF NOT EXISTS idx_alarm_severity ON alarms (severity) TYPE set(100) GRANULARITY 1;
CREATE INDEX IF NOT EXISTS idx_alarm_ack ON alarms (acknowledged) TYPE set(2) GRANULARITY 4;

-- ============================================================
-- 材质视图：供可视化查询
-- ============================================================
CREATE VIEW IF NOT EXISTS v_drum_material_summary
AS
SELECT
    drum_id,
    toDate(timestamp) AS date,
    avg(copper_pct) AS avg_copper,
    avg(tin_pct) AS avg_tin,
    avg(lead_pct) AS avg_lead,
    stddevPop(copper_pct) AS std_copper,
    stddevPop(tin_pct) AS std_tin,
    count() AS sample_count
FROM sensor_readings
GROUP BY drum_id, toDate(timestamp)
ORDER BY drum_id, date DESC;

-- ============================================================
-- 告警统计视图
-- ============================================================
CREATE VIEW IF NOT EXISTS v_alarm_summary
AS
SELECT
    drum_id,
    toStartOfDay(timestamp) AS day,
    severity,
    alarm_type,
    count() AS alarm_count,
    avg(measured_value) AS avg_measured,
    max(measured_value) AS max_measured
FROM alarms
GROUP BY drum_id, toStartOfDay(timestamp), severity, alarm_type
ORDER BY day DESC, drum_id, severity;

-- ============================================================
-- 初始化示例数据：插入几面典型的西南少数民族铜鼓
-- ============================================================
INSERT INTO drums (drum_id, name, ethnic_group, origin_region, estimated_era, diameter_cm, height_cm, mass_kg, created_at, notes) VALUES
('drum-001-zhuang', '左江花山型铜鼓', '壮族', '广西宁明县', '西汉-东汉', 78.5, 52.3, 38.5, now64(9), '典型冷水冲型，面饰羽人划船纹'),
('drum-002-miao', '雷山型大铜鼓', '苗族', '贵州雷山县', '宋代-明代', 112.0, 76.0, 82.0, now64(9), '麻江型重器，十二芒太阳纹'),
('drum-003-dong', '从江侗族铜鼓', '侗族', '贵州从江县', '清代', 56.2, 38.0, 22.5, now64(9), '北流型变体，双环耳'),
('drum-004-bouyei', '黔南布依铜鼓', '布依族', '贵州黔南州', '明-清', 65.8, 44.5, 31.2, now64(9), '遵义型，蛙饰四立蛙'),
('drum-005-yi', '楚雄万家坝型', '彝族', '云南楚雄市', '春秋战国', 45.0, 28.0, 15.8, now64(9), '早期原始铜鼓，素面为主');

-- ============================================================
-- 物化视图（可选）：传感器最新状态
-- ============================================================
-- CREATE MATERIALIZED VIEW IF NOT EXISTS mv_latest_sensor
-- ENGINE = ReplacingMergeTree(timestamp)
-- ORDER BY drum_id
-- POPULATE
-- AS
-- SELECT drum_id, max(timestamp) AS timestamp, any(sensor_ids) AS sensor_ids
-- FROM sensor_readings
-- GROUP BY drum_id;

-- ============================================================
-- MQTT 表引擎示例（可选，当使用 MQTT 桥接时）
-- ============================================================
-- CREATE TABLE IF NOT EXISTS mqtt_sensor_queue (
--     payload String
-- )
-- ENGINE = MQTT
-- SETTINGS
--   mqtt_host_port = '127.0.0.1:1883',
--   mqtt_username = '',
--   mqtt_password = '',
--   mqtt_topic = 'bronze-drum/sensors/#',
--   mqtt_format = 'JSONEachRow';
