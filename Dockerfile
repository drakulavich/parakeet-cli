FROM oven/bun:1.3.13-slim

WORKDIR /app

ENV NODE_ENV=production \
    KESHA_CACHE_DIR=/cache/kesha

COPY package.json bun.lock ./
RUN bun install --frozen-lockfile --production --ignore-scripts

COPY bin ./bin
COPY src ./src
COPY fixtures/benchmark ./fixtures/benchmark
COPY fixtures/benchmark-en ./fixtures/benchmark-en
COPY BENCHMARK.md LICENSE NOTICES.md README.md SKILL.md ./
COPY openclaw.plugin.json openclaw-plugin.cjs ./

RUN chmod +x /app/bin/kesha.js \
  && ln -s /app/bin/kesha.js /usr/local/bin/kesha \
  && ln -s /app/bin/kesha.js /usr/local/bin/parakeet \
  && mkdir -p /cache/kesha /work \
  && chown -R bun:bun /app /cache /work

USER bun
WORKDIR /work

ENTRYPOINT ["kesha"]
CMD ["--help"]
