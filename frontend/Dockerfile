FROM oven/bun:1 AS base
WORKDIR /usr/src/app

# Install dependencies into temp directory
FROM base AS install
RUN mkdir -p /temp/dev
COPY ./package.json ./bun.lock /temp/dev/
RUN cd /temp/dev && bun install --frozen-lockfile

# Copy production dependencies and source code into final image
FROM base AS release
COPY --from=install /temp/dev/node_modules node_modules
COPY ./ ./
COPY --from=shared ./sql/ugoite-sql-rules.json /usr/src/shared/sql/ugoite-sql-rules.json
COPY --from=shared ./i18n/ui.json /usr/src/shared/i18n/ui.json

# Build the frontend application
RUN bun run build

# Run the app
USER bun
EXPOSE 3000/tcp
ENTRYPOINT [ "bun", "run", "start" ]
