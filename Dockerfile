FROM python:3.12-slim

RUN apt-get update && apt-get install -y --no-install-recommends cron \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/backup-system

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY app ./app
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh

ENV PYTHONUNBUFFERED=1 \
    PYTHONPATH=/opt/backup-system

ENTRYPOINT ["./entrypoint.sh"]
