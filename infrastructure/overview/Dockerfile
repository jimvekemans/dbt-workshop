FROM python:3.13.18-slim

RUN pip install --no-cache-dir pymysql==1.1.1

WORKDIR /app
COPY app.py index.html services.json ./

# Runs as root on purpose: /var/run/docker.sock is root:docker on the host and the docker group id differs per host,
# so a non-root user could not read container state. The socket is mounted read-only and only GET is ever sent.
EXPOSE 8000
CMD ["python", "-u", "app.py"]
