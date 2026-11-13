// @ts-check

/**
 * Puts a new password into a sign-up or password-change form.
 *
 * Runs inside the page, through `chrome.scripting.executeScript`, only after
 * the person asks for a password in the popup. Like `fillPage`, it must not
 * refer to anything outside its own body, it never submits, and it types only
 * into fields a person can see.
 *
 * Called with `password` null, it types nothing and reports the form: its
 * address and the username already typed, which Vardo files the password
 * under. With a password, it fills every new-password field, so the
 * confirmation box matches.
 *
 * @param {string | null} expectedOrigin The origin Vardo made the password for.
 * @param {string | null} password
 * @returns {{ url: string, username: string, fields: number } | { reason: string }}
 */
export function signUpPage(expectedOrigin, password) {
	if (expectedOrigin !== null && location.origin !== expectedOrigin) return { reason: "wrong-origin" };
	if (window !== window.top) return { reason: "frame" };

	/** @param {HTMLInputElement} el */
	const visible = (el) => {
		if (el.disabled || el.readOnly || el.type === "hidden") return false;
		const rect = el.getBoundingClientRect();
		if (rect.width < 4 || rect.height < 4) return false;
		let opacity = 1;
		for (let node = /** @type {Element | null} */ (el); node; node = node.parentElement) {
			const style = getComputedStyle(node);
			if (style.display === "none" || style.visibility === "hidden") return false;
			opacity *= Number.parseFloat(style.opacity || "1");
		}
		return opacity >= 0.3;
	};
	/** @param {HTMLInputElement} el */
	const autocomplete = (el) => (el.getAttribute("autocomplete") || "").toLowerCase().split(/\s+/);

	const inputs = /** @type {HTMLInputElement[]} */ ([...document.querySelectorAll("input")]).filter(visible);
	const passwords = inputs.filter((el) => el.type === "password");
	const marked = passwords.filter((el) => autocomplete(el).includes("new-password"));
	// Unmarked forms: a sign-up shows a password and its confirmation; a
	// password change adds the current one first, which must stay untouched.
	/** @type {HTMLInputElement[]} */
	let fresh = [];
	if (marked.length > 0) fresh = marked;
	else if (passwords.length === 2) fresh = passwords;
	else if (passwords.length === 3) fresh = passwords.slice(1);
	if (fresh.length === 0) return { reason: "no-sign-up" };

	const first = fresh[0];
	const scope = first.form ? inputs.filter((el) => el.form === first.form) : inputs;
	const named = scope.find((el) => autocomplete(el).some((t) => t === "username" || t === "email"));
	const before = scope.filter(
		(el) =>
			["text", "email", "tel"].includes(el.type) &&
			el.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING,
	);
	const username = (named ?? before.at(-1))?.value.trim() ?? "";

	if (password !== null) {
		const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
		for (const el of fresh) {
			el.focus();
			if (setter) setter.call(el, password);
			else el.value = password;
			el.dispatchEvent(new Event("input", { bubbles: true }));
			el.dispatchEvent(new Event("change", { bubbles: true }));
		}
	}
	return { url: location.href, username, fields: fresh.length };
}
