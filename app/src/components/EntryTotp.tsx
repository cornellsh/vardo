import { useEffect, useState } from "react";

import { ipc } from "../ipc";
import type { Uuid } from "../ipc";
import { TotpCode } from "./TotpCode";
import { GroupHeading } from "./EntryField";

export function TotpRow({ uuid }: { uuid: Uuid }) {
	const [present, setPresent] = useState(false);

	useEffect(() => {
		let cancelled = false;
		void (async () => {
			try {
				const code = await (await ipc()).totp(uuid);
				if (!cancelled) setPresent(code !== null);
			} catch {
				if (!cancelled) setPresent(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [uuid]);

	if (!present) return null;

	return (
		// The rule sits below its own padding and above a margin, so the first
		// field's highlight has somewhere to be. Flush against the border it read
		// as the hover box colliding with the line rather than sitting under it.
		<div
			className="mb-3 flex items-start gap-2 pt-1 pb-3"
			style={{ borderBottom: "1px solid var(--shadow-fg)" }}
		>
			<div className="min-w-0 flex-1">
				<GroupHeading>One-time code</GroupHeading>
				<TotpCode
					uuid={uuid}
					onCopy={() => {
						// Copying the code goes through the same Rust clipboard path
						// as a password, so it is auto-cleared on the same schedule.
						void (async () => {
							try {
								await (await ipc()).copyField(uuid, "otp");
							} catch {
								/* nothing to copy */
							}
						})();
					}}
				/>
			</div>
		</div>
	);
}

/**
 * The label under a value.
 *
 * Under, not over, and not shouted in capitals. Telegram's info panel puts the
 * value first because that is what you came to read - the label only tells you
 * what you are already looking at. Uppercase tracking-wide headers are the iOS
 * settings idiom, and using them here made every field look like a form.
 */
/**
 * A heading above a group of rows, as opposed to the label under one value.
 * Still not shouted: weight and colour carry it.
 */
/**
 * One attachment, with a way to get it out.
 *
 * It used to be a filename and a byte count and nothing else. People put
 * recovery codes and SSH keys in here precisely because they matter, and a
 * vault that can show you the name of the thing you need but never the thing
 * itself is somewhere data goes to be lost.
 */
