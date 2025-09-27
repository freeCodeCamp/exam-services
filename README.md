# Exam Services

```bash
cd moderation-service && cp sample.env .env
```

```bash
docker compose up -d
```

## Deployment

This project is deployed on a Digital Ocean Droplet.

In the VM, set the environment variables, build the image(s), then move the `cron` file into the crontab:

```bash
git clone https://github.com/freeCodeCamp/exam-services.git
cd exam-services
# Depending on the environment, add variables to .env.production or .env.staging
cp moderation-service/sample.env moderation-service/.env.<environment>
docker compose build
crontab cron.<environment>
```

The VM has a 4GB Swap to enable local builds on 1GB machines.

<details>
  <summary>Swap Setup</summary>

```bash
sudo fallocate -l 4G /swapfile
# Only root may use
sudo chmod 600 /swapfile
sudo mkswap /swapfile
sudo swapon /swapfile
```

</details>

## Moderation Service

### Development

```bash
docker compose up -d moderation
```

### Testing

Seed database with exam and attempt

```bash
cd freeCodeCamp/freeCodeCampm
pnpm run seed:exam-env --attempt
```

A specific version of `rustc` is used, because the test tooling requires nightly features.

```bash
cargo +nightly-2025-04-03 test
```
