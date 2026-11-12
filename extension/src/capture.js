// @ts-check

/**
 * Reads the login a person just typed, so Vardo can save it.
 *
 * Runs inside the page through `chrome.scripting.executeScript`, only after the
 * person clicks "Save this login" in the popup (`inv:extension-sees-nothing-until-asked`).
 * Chrome sends this function's source and nothing around it, so it must not
 * refer to anything outside its own body.
 *
 * On a change-password form it takes the new password: the field marked
 * `autocomplete="new-password"`, else the last of two matching fields.
 *
 * @returns {{ url: string, username: string, password: string } | { reason: string }}
 */
export function capturePage() {
	/** @param {HTMLInputElement} el */
	const visible = (el) => {
		if (el.disabled || el.type === "hidden") return false;
		const rect = el.getBoundingClientRect();
		if (rect.width < 4 || rect.height < 4) return false;
		for (let node = /** @type {Element | null} */ (el); node; node = node.parentElement) {
			const style = getComputedStyle(node);
			if (style.display === "none" || style.visibility === "hidden") return false;
		}
		return true;
	};
	/** @param {HTMLInputElement} el */
	const autocomplete = (el) => (el.getAttribute("autocomplete") || "").toLowerCase().split(/\s+/);
	/** @param {ParentNode} root */
	const inputs = (root) =>
		/** @type {HTMLInputElement[]} */ ([...root.querySelectorAll("input")]).filter(visible);

	const filled = inputs(document).filter((el) => el.type === "password" && el.value);
	if (filled.length === 0) return { reason: "no-password" };

	// The form the person is in wins over any other form on the page.
	const active = document.activeElement instanceof HTMLInputElement ? document.activeElement.form : null;
	const form = active ?? filled[0].form;
	const scope = form ?? document;
	const passwords = filled.filter((el) => el.form === form);

	const marked = passwords.find((el) => autocomplete(el).includes("new-password"));
	let password = marked ?? passwords[0];
	if (!marked && passwords.length >= 2) {
		const [a, b] = passwords.slice(-2);
		if (a.value === b.value) password = b;
	}

	const fields = inputs(scope);
	const textual = fields.filter((el) => ["text", "email", "tel"].includes(el.type) && el.value);
	const named = textual.find((el) => autocomplete(el).some((t) => t === "username" || t === "email"));
	// Otherwise the last text field before the password, as on nearly every sign-in form.
	const before = textual.filter(
		(el) => el.compareDocumentPosition(password) & Node.DOCUMENT_POSITION_FOLLOWING,
	);
	const username = (named ?? before.at(-1))?.value.trim() ?? "";

	return { url: location.href, username, password: password.value };
}
