// @ts-check

/**
 * Types a login into the page.
 *
 * Runs inside the page, through `chrome.scripting.executeScript`. Chrome sends
 * this function's source text and nothing around it, so it must not refer to
 * anything outside its own body: no imports, no helpers defined elsewhere.
 *
 * It never submits. The person presses Enter or clicks the button, which is the
 * last chance to notice they are somewhere they did not mean to be.
 *
 * Called with `login` null, it types nothing and reports what it would fill.
 * The extension asks every frame that way to find the one with the form.
 *
 * @param {string | null} expectedOrigin The origin Vardo approved this login for.
 * @param {{ username: string, password: string, totp: string | null } | null} login
 * @returns {{ filled: string[], reason?: string }}
 */
export function fillPage(expectedOrigin, login) {
	// The page may have navigated between Vardo approving the login and this
	// running. Chrome revokes access on a cross-site navigation, but the check
	// costs nothing and does not depend on that.
	if (expectedOrigin !== null && location.origin !== expectedOrigin) {
		return { filled: [], reason: "wrong-origin" };
	}

	/**
	 * Visible to a person, not merely present in the document.
	 *
	 * The 2025 clickjacking attacks collected logins through forms made almost
	 * transparent or pushed off screen. Filling only what someone could see
	 * closes that, whatever triggered the fill.
	 *
	 * @param {HTMLInputElement} el
	 */
	const visible = (el) => !el.disabled && !el.readOnly && el.type !== "hidden" && shown(el);

	/**
	 * Shown on screen in its own document. Styles and scroll are read from that
	 * document's window, so this also works for an iframe element as seen by
	 * its parent.
	 *
	 * @param {Element} el
	 */
	const shown = (el) => {
		const view = el.ownerDocument.defaultView;
		if (!view) return false;
		const rect = el.getBoundingClientRect();
		if (rect.width < 4 || rect.height < 4) return false;
		// Pushed off the left or top of the document, where no scrolling can
		// reach it.
		if (rect.right + view.scrollX <= 0 || rect.bottom + view.scrollY <= 0) return false;
		let opacity = 1;
		for (let node = /** @type {Element | null} */ (el); node; node = node.parentElement) {
			const style = view.getComputedStyle(node);
			if (style.display === "none") return false;
			if (style.visibility === "hidden" || style.visibility === "collapse") return false;
			opacity *= Number.parseFloat(style.opacity || "1");
		}
		return opacity >= 0.3;
	};

	// Everything below sees only this document, so inside a frame a hidden
	// iframe would hide a form that looks visible from here. Every frame up to
	// the top must be shown in its parent. A parent this frame cannot look into
	// is another site, and is refused rather than trusted.
	for (let w = /** @type {Window} */ (window); w !== w.top; w = /** @type {Window} */ (w.parent)) {
		/** @type {Element | null} */
		let frame = null;
		try {
			frame = w.frameElement;
		} catch {
			frame = null;
		}
		if (!frame || !shown(frame)) return { filled: [], reason: "hidden-frame" };
	}

	/** @param {HTMLInputElement} el */
	const autocomplete = (el) => (el.getAttribute("autocomplete") || "").toLowerCase().split(/\s+/);

	/** @param {HTMLInputElement} el */
	const describe = (el) =>
		[el.name, el.id, el.placeholder, el.getAttribute("aria-label") || ""].join(" ").toLowerCase();

	/** @param {HTMLInputElement} el */
	const textual = (el) => ["text", "email", "tel", "number"].includes(el.type);

	/** @param {HTMLInputElement} el */
	const looksLikeCode = (el) =>
		autocomplete(el).includes("one-time-code") ||
		/(^|[^a-z])(otp|totp|2fa|mfa|one.?time|verification|auth.?code|security.?code|passcode)/.test(
			describe(el),
		);

	/**
	 * Sets a value the way typing would, so frameworks that keep their own copy
	 * of the field (React, Vue, Angular) notice. Assigning `.value` alone is
	 * invisible to React, which then submits the empty string it remembered.
	 *
	 * @param {HTMLInputElement} el
	 * @param {string} value
	 */
	const type = (el, value) => {
		if (!login) return;
		el.focus();
		const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
		if (setter) setter.call(el, value);
		else el.value = value;
		el.dispatchEvent(new Event("input", { bubbles: true }));
		el.dispatchEvent(new Event("change", { bubbles: true }));
	};

	const inputs = /** @type {HTMLInputElement[]} */ (Array.from(document.querySelectorAll("input"))).filter(
		visible,
	);
	const passwords = inputs.filter((el) => el.type === "password");

	// A form that only asks for a new password is a sign-up or a password
	// change. Putting the existing password there would be wrong in both.
	const current = passwords.filter((el) => !autocomplete(el).includes("new-password"));
	if (passwords.length > 0 && current.length === 0) {
		return { filled: [], reason: "new-password" };
	}
	const password =
		current.find((el) => autocomplete(el).includes("current-password")) || current[0] || null;

	/** @type {string[]} */
	const filled = [];

	if (password) {
		const scope = password.form
			? inputs.filter((el) => el.form === password.form)
			: inputs;
		const before = scope.filter(
			(el) =>
				textual(el) &&
				!looksLikeCode(el) &&
				el.compareDocumentPosition(password) & Node.DOCUMENT_POSITION_FOLLOWING,
		);
		const username =
			before.find((el) => autocomplete(el).some((t) => t === "username" || t === "email")) ||
			before.find((el) => el.type === "email") ||
			before[before.length - 1] ||
			null;

		if (username && (!login || login.username)) {
			type(username, login?.username ?? "");
			filled.push("username");
		}
		type(password, login?.password ?? "");
		filled.push("password");
		// Leave the cursor in the password field, so Enter signs in.
		if (login) password.focus();
		return { filled };
	}

	// No password field. Either the first step of a two-step sign-in, asking
	// only for the username, or the step after, asking for a code.
	const codes = inputs.filter((el) => textual(el) && looksLikeCode(el));
	if (!login || login.totp) {
		if (codes.length === 1) {
			type(codes[0], login?.totp ?? "");
			return { filled: ["totp"] };
		}
		// One box per digit. Typed one digit each, because these forms usually
		// move the focus along as each box fills.
		const boxes = inputs.filter((el) => textual(el) && el.maxLength === 1);
		if (login?.totp && boxes.length === login.totp.length) {
			boxes.forEach((box, i) => type(box, login.totp?.[i] ?? ""));
			return { filled: ["totp"] };
		}
	}

	const identifier =
		inputs.find((el) => autocomplete(el).some((t) => t === "username" || t === "email")) ||
		inputs.find((el) => el.type === "email") ||
		inputs.find((el) => textual(el) && /user|email|login|account|identifier/.test(describe(el)));
	if (identifier && (!login || login.username)) {
		type(identifier, login?.username ?? "");
		return { filled: ["username"] };
	}

	return { filled: [], reason: "no-fields" };
}
