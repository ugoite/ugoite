module.exports = {
  extends: ["@commitlint/config-conventional"],
  ignores: [
    (message) => /^chore\(deps(?:-dev)?\): Bump .+/u.test(message),
  ],
  rules: {
    "body-max-line-length": [0, "always", 100],
  },
};
