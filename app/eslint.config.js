import tsParser from "@typescript-eslint/parser";
import jsxA11y from "eslint-plugin-jsx-a11y";

// `inv:keyboard-reachable`: the plugin's recommended accessibility rules, which
// are all errors. Types and style are tsc's and the formatter's job.
export default [
	{
		files: ["src/**/*.{ts,tsx}"],
		languageOptions: {
			parser: tsParser,
			parserOptions: { ecmaFeatures: { jsx: true } },
		},
		linterOptions: { reportUnusedDisableDirectives: "error" },
		plugins: { "jsx-a11y": jsxA11y },
		rules: {
			...jsxA11y.flatConfigs.recommended.rules,
			// Our own <Input> forwards autoFocus to a DOM input, which is checked there.
			"jsx-a11y/no-autofocus": ["error", { ignoreNonDOM: true }],
			// Radio rows nest the label text one span deeper than the default 2.
			"jsx-a11y/label-has-associated-control": ["error", { depth: 3 }],
		},
	},
];
