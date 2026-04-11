import tseslint from "typescript-eslint";

export default tseslint.config(
  ...tseslint.configs.recommended,
  {
    rules: {
      "max-depth": ["warn", { max: 4 }],
    },
  },
);
