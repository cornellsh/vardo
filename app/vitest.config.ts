import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

/**
 * Two environments, chosen per file.
 *
 * The pure-logic tests want no DOM at all and run in milliseconds. The
 * component tests need one, and jsdom costs about a second to stand up. Making
 * every file pay for that would be the sort of tax that eventually gets a test
 * suite skipped.
 */
export default defineConfig({
	plugins: [react()],
	test: {
		globals: false,
		restoreMocks: true,
		projects: [
			{
				extends: true,
				test: {
					name: "logic",
					environment: "node",
					include: ["src/**/*.test.ts"],
				},
			},
			{
				extends: true,
				test: {
					name: "dom",
					environment: "jsdom",
					include: ["src/**/*.dom.test.tsx"],
				},
			},
		],
	},
});
