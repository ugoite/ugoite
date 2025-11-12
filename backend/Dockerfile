FROM python:3.12-slim-trixie

WORKDIR /app

# Install uv
COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /bin/

# Copy pyproject.toml and uv.lock for dependency installation
COPY ./pyproject.toml .
COPY ./uv.lock .
COPY ./README.md .
COPY --from=module ./ ../ieapp-cli/

# Copy the rest of the backend code
COPY ./src src

# Install backend dependencies
RUN uv sync --locked

# Set the entrypoint for the backend application
CMD ["uvicorn", "src.backend.main:app", "--host", "0.0.0.0", "--port", "8000"]
