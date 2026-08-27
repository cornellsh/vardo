/**
 * Turns whatever `invoke` rejected with into something a person can read.
 *
 * Written because the obvious `String(err.detail ?? err)` renders a plain
 * object as `[object Object]` — which is strictly worse than no message,
 * because it looks like a bug in the app rather than a description of one. A
 * `UiError` variant with no `detail` field (`wrong-password`, `locked`) hits
 * that path every time.
 */

const FRIENDLY: Record<string, string> = {
	"wrong-password": "Wrong password.",
	"not-found": "That item no longer exists.",
	locked: "The vault is locked.",
};

export function errorText(err: unknown): string {
	if (typeof err === "string") return err;
	if (err instanceof Error) return err.message;

	if (err && typeof err === "object") {
		const e = err as { kind?: string; detail?: string; path?: string };
		if (typeof e.detail === "string" && e.detail) return e.detail;
		if (typeof e.kind === "string") {
			const friendly = FRIENDLY[e.kind];
			if (friendly) return friendly;
			return e.path ? `${e.kind}: ${e.path}` : e.kind;
		}
		// Last resort, but still informative rather than "[object Object]".
		try {
			return JSON.stringify(err);
		} catch {
			return "Unknown error.";
		}
	}

	return String(err);
}
