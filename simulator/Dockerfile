FROM python:3.11-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY simulator_config.json .
COPY drum_simulator.py .

RUN chmod +x drum_simulator.py

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD python -c "import requests; r=requests.get('http://backend:8080/api/health', timeout=3); exit(0 if r.status_code==200 else 1)"

ENTRYPOINT ["python", "drum_simulator.py"]
CMD ["--interval", "3600"]
