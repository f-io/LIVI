const tseslint = require("typescript-eslint");

module.exports = [
  {
    ignores: [
      "node_modules",
      "dist",
      "out",
      ".gitignore"
    ],
  },
  ...tseslint.configs.recommended,
  {
    rules: {
      "@typescript-eslint/no-explicit-any": "off", // TODO : Review usage of 'any' and enable this rule
      "@typescript-eslint/no-require-imports": "off",
    },
  },
];