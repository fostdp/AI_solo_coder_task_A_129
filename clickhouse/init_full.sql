-- ============================================================
-- 古代铜鼓系统 ClickHouse 初始化脚本（完整版）
-- 包含：建库建表 + 降采样物化视图 + TTL 保留策略 + 示例数据
-- ============================================================

CREATE DATABASE IF NOT EXISTS bronze_drum
    ENGINE = Atomic
    COMMENT '古代铜鼓系统数据库';

USE bronze_drum;

-- ============================================================
-- 1. 铜鼓主表（MergeTree，保留策略：永久）
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
ENGINE = ReplacingMergeTree(created_at)
PARTITION BY toYYYYMM(created_at)
ORDER BY (drum_id, created_at)
PRIMARY KEY drum_id
COMMENT '铜鼓档案表(ReplacingMergeTree按最新覆盖)';

-- ============================================================
-- 2. 传感器读数表（小时级精细数据，TTL: 30天 → 自动删除）
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
TTL timestamp + INTERVAL 30 DAY
COMMENT '传感器采集数据表(精细粒度，保留30天)';

CREATE INDEX IF NOT EXISTS idx_sensor_drum ON sensor_readings (drum_id) TYPE minmax GRANULARITY 1;
CREATE INDEX IF NOT EXISTS idx_sensor_ts ON sensor_readings (timestamp) TYPE minmax GRANULARITY 4;

-- ============================================================
-- 2.1 降采样：传感器读数 - 每小时聚合表（TTL: 6个月）
-- ============================================================
CREATE TABLE IF NOT EXISTS sensor_readings_hourly (
    drum_id         String,
    hour_bucket     DateTime,
    samples         UInt64              COMMENT '小时内样本数',
    avg_copper      Float64             COMMENT '小时平均铜含量',
    min_copper      Float64,
    max_copper      Float64,
    std_copper      Float64,
    avg_tin         Float64             COMMENT '小时平均锡含量',
    min_tin         Float64,
    max_tin         Float64,
    avg_lead        Float64             COMMENT '小时平均铅含量',
    avg_temp_c      Float64             COMMENT '小时平均温度',
    min_temp_c      Float64,
    max_temp_c      Float64,
    avg_humidity_pct Float64
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(hour_bucket)
ORDER BY (drum_id, hour_bucket)
PRIMARY KEY (drum_id, hour_bucket)
TTL hour_bucket + INTERVAL 6 MONTH
COMMENT '传感器读数-小时聚合(保留6个月)';

-- 物化视图：从 sensor_readings 自动写入小时聚合
CREATE MATERIALIZED VIEW IF NOT EXISTS mv_sensor_hourly
TO sensor_readings_hourly
AS
SELECT
    drum_id,
    toStartOfHour(timestamp) AS hour_bucket,
    count()              AS samples,
    avg(copper_pct)      AS avg_copper,
    min(copper_pct)      AS min_copper,
    max(copper_pct)      AS max_copper,
    stddevPop(copper_pct) AS std_copper,
    avg(tin_pct)         AS avg_tin,
    min(tin_pct)         AS min_tin,
    max(tin_pct)         AS max_tin,
    avg(lead_pct)        AS avg_lead,
    avg(temperature_c)   AS avg_temp_c,
    min(temperature_c)   AS min_temp_c,
    max(temperature_c)   AS max_temp_c,
    avg(humidity_pct)    AS avg_humidity_pct
FROM sensor_readings
GROUP BY drum_id, toStartOfHour(timestamp);

-- ============================================================
-- 2.2 降采样：传感器读数 - 每天聚合表（TTL: 2年）
-- ============================================================
CREATE TABLE IF NOT EXISTS sensor_readings_daily (
    drum_id         String,
    day_bucket      Date,
    samples         UInt64,
    avg_copper      Float64,
    min_copper      Float64,
    max_copper      Float64,
    std_copper      Float64,
    avg_tin         Float64,
    avg_lead        Float64,
    avg_temp_c      Float64,
    min_temp_c      Float64,
    max_temp_c      Float64
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(day_bucket)
ORDER BY (drum_id, day_bucket)
PRIMARY KEY (drum_id, day_bucket)
TTL day_bucket + INTERVAL 2 YEAR
COMMENT '传感器读数-天聚合(保留2年)';

CREATE MATERIALIZED VIEW IF NOT EXISTS mv_sensor_daily
TO sensor_readings_daily
AS
SELECT
    drum_id,
    toDate(hour_bucket)  AS day_bucket,
    sum(samples)         AS samples,
    avg(avg_copper)      AS avg_copper,
    min(min_copper)      AS min_copper,
    max(max_copper)      AS max_copper,
    avg(std_copper)      AS std_copper,
    avg(avg_tin)         AS avg_tin,
    avg(avg_lead)        AS avg_lead,
    avg(avg_temp_c)      AS avg_temp_c,
    min(min_temp_c)      AS min_temp_c,
    max(max_temp_c)      AS max_temp_c
FROM sensor_readings_hourly
GROUP BY drum_id, toDate(hour_bucket);

-- ============================================================
-- 3. 壁厚历史明细表（TTL: 1年）
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
TTL timestamp + INTERVAL 1 YEAR
COMMENT '壁厚测点历史(保留1年)';

-- ============================================================
-- 4. 铸造仿真结果表（TTL: 永久）
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
    overall_risk    LowCardinality(String) COMMENT '整体风险等级',
    niyama_min      Float64             COMMENT '最小Niyama判据值',
    solidification_s Float64            COMMENT '总凝固时间(秒)'
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(created_at)
ORDER BY (drum_id, created_at, sim_id)
PRIMARY KEY (drum_id, created_at)
COMMENT '铸造工艺仿真结果(永久保留)';

-- 铸造仿真月度聚合
CREATE TABLE IF NOT EXISTS casting_sim_monthly (
    drum_id         String,
    month_bucket    Date,
    sim_count       UInt64,
    avg_quality     Float64,
    min_quality     Float64,
    defect_count    UInt64,
    avg_niyama_min  Float64
)
ENGINE = SummingMergeTree()
PARTITION BY toYYYYMM(month_bucket)
ORDER BY (drum_id, month_bucket)
PRIMARY KEY (drum_id, month_bucket)
COMMENT '铸造仿真月度统计(SummingMergeTree自动聚合)';

CREATE MATERIALIZED VIEW IF NOT EXISTS mv_casting_monthly
TO casting_sim_monthly
AS
SELECT
    drum_id,
    toStartOfMonth(created_at) AS month_bucket,
    1 AS sim_count,
    quality_score AS avg_quality,
    quality_score AS min_quality,
    length(JSONExtractKeys(defects)) AS defect_count,
    niyama_min AS avg_niyama_min
FROM casting_simulations;

-- ============================================================
-- 5. 声学分析结果表（TTL: 永久）
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
    sound_quality   Float64             COMMENT '声学品质评分(0-1)',
    freq_deviation_hz Float64           COMMENT '频率偏差(Hz)'
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(created_at)
ORDER BY (drum_id, created_at, analysis_id)
PRIMARY KEY (drum_id, created_at)
COMMENT '声学特性有限元分析结果(永久保留)';

-- ============================================================
-- 6. 告警表（TTL: 1年归档，原表保留2年）
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
COMMENT '告警历史表(保留2年)';

CREATE INDEX IF NOT EXISTS idx_alarm_severity ON alarms (severity) TYPE set(100) GRANULARITY 1;
CREATE INDEX IF NOT EXISTS idx_alarm_ack ON alarms (acknowledged) TYPE set(2) GRANULARITY 4;
CREATE INDEX IF NOT EXISTS idx_alarm_type ON alarms (alarm_type) TYPE set(100) GRANULARITY 1;

-- 告警统计聚合（每小时，保留5年）
CREATE TABLE IF NOT EXISTS alarm_stats_hourly (
    hour_bucket     DateTime,
    drum_id         String,
    severity        LowCardinality(String),
    alarm_type      LowCardinality(String),
    count           UInt64,
    avg_measured    Float64,
    max_measured    Float64
)
ENGINE = SummingMergeTree()
PARTITION BY toYYYYMM(hour_bucket)
ORDER BY (hour_bucket, drum_id, severity, alarm_type)
TTL hour_bucket + INTERVAL 5 YEAR
COMMENT '告警小时统计(SummingMergeTree自动汇总，保留5年)';

CREATE MATERIALIZED VIEW IF NOT EXISTS mv_alarm_stats_hourly
TO alarm_stats_hourly
AS
SELECT
    toStartOfHour(timestamp) AS hour_bucket,
    drum_id,
    severity,
    alarm_type,
    1 AS count,
    measured_value AS avg_measured,
    measured_value AS max_measured
FROM alarms;

-- ============================================================
-- 7. 物化视图（查询加速）
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
GROUP BY drum_id, toDate(timestamp);

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
GROUP BY drum_id, toStartOfDay(timestamp), severity, alarm_type;

-- 当前告警看板（未确认的严重告警）
CREATE VIEW IF NOT EXISTS v_active_critical_alarms
AS
SELECT
    alarm_id, drum_id, timestamp, alarm_type, severity,
    message, measured_value, threshold_value
FROM alarms
WHERE acknowledged = 0 AND severity IN ('Critical', 'Fatal')
ORDER BY timestamp DESC
LIMIT 500;

-- ============================================================
-- 8. 示例数据
-- ============================================================
INSERT INTO drums (drum_id, name, ethnic_group, origin_region, estimated_era, diameter_cm, height_cm, mass_kg, created_at, notes) VALUES
('drum-001-zhuang', '左江花山型铜鼓', '壮族', '广西宁明县', '西汉-东汉', 78.5, 52.3, 38.5, now64(9), '典型冷水冲型，面饰羽人划船纹'),
('drum-002-miao', '雷山型大铜鼓', '苗族', '贵州雷山县', '宋代-明代', 112.0, 76.0, 82.0, now64(9), '麻江型重器，十二芒太阳纹'),
('drum-003-dong', '从江侗族铜鼓', '侗族', '贵州从江县', '清代', 56.2, 38.0, 22.5, now64(9), '北流型变体，双环耳'),
('drum-004-bouyei', '黔南布依铜鼓', '布依族', '贵州黔南州', '明-清', 65.8, 44.5, 31.2, now64(9), '遵义型，蛙饰四立蛙'),
('drum-005-yi', '楚雄万家坝型', '彝族', '云南楚雄市', '春秋战国', 45.0, 28.0, 15.8, now64(9), '早期原始铜鼓，素面为主');

-- ============================================================
-- 9. 系统配置：用户和配额（可选）
-- ============================================================
-- CREATE USER IF NOT EXISTS bronze_app IDENTIFIED BY 'AppPassword_CHANGE_ME';
-- GRANT SELECT, INSERT ON bronze_drum.* TO bronze_app;
-- GRANT SELECT ON system.* TO bronze_app;

-- 写入配额保护
-- SET PROFILE bronze_app_profile = 'default';
