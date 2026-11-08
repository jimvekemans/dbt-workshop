# Workshop Studio: React frontend (built once) + FastAPI backend in one small image.
# Needs the Docker socket (docker exec airflow-*), the GitLab PAT and the SingleStore root password -
# see docker-compose.yml, which publishes it on 127.0.0.1:8085 only. Never expose it.
FROM node:24-alpine AS web
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
# npm ci: exactly the versions in package-lock.json (reproducible image)
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
RUN npm run build

FROM python:3.13-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends git docker-cli ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY backend/requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt
COPY backend/app ./app
COPY scenarios ./scenarios
COPY patches ./patches
COPY --from=web /web/dist ./static
ENV STUDIO_HOST=0.0.0.0 \
    STUDIO_PORT=8085 \
    PYTHONUNBUFFERED=1
EXPOSE 8085
HEALTHCHECK --interval=15s --timeout=3s --retries=5 \
    CMD python -c "import urllib.request as u; u.urlopen('http://127.0.0.1:8085/api/state', timeout=2)"
CMD ["python", "-m", "app.main"]
