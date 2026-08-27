import { useCallback, useEffect, useState } from "react";

import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { GeneratorOptions } from "../ipc";

interface Props {
	onUse: (password: string) => void;
	onClose: () => void;
}

const DEFAULTS: GeneratorOptions = {
	lowercase: true,
	uppercase: true,
	digits: true,
	symbols: true,
	excludeAmbiguous: false,
};

/**
 * The generated value is a secret and lives in component state, cleared on
 * unmount. Generation happens in Rust — see `Ipc.generatePassword`.
 */
export function PasswordGenerator({ onUse, onClose }: Props) {
	const [length, setLength] = useState(20);
	const [options, setOptions] = useState<GeneratorOptions>(DEFAULTS);
	const [value, setValue] = useState("");
	const [error, setError] = useState<string | null>(null);

	const regenerate = useCallback(async () => {
		try {
			setValue(await (await ipc()).generatePassword(length, options));
			setError(null);
		} catch (err) {
			setError(errorText(err));
			setValue("");
		}
	}, [length, options]);

	useEffect(() => {
		void regenerate();
	}, [regenerate]);

	// A generated password must not outlive this component.
	useEffect(() => () => setValue(""), []);

	const toggle = (key: keyof GeneratorOptions) =>
		setOptions((o) => ({ ...o, [key]: !o[key] }));

	return (
		<div
			className="rounded-[6px] p-3"
			style={{
				background: "var(--window-bg-over)",
				border: "1px solid var(--shadow-fg)",
			}}
		>
			<div className="mb-2 flex items-center gap-2">
				<output
					className="secret min-w-0 flex-1 truncate rounded-[4px] px-2 py-1.5"
					style={{
						background: "var(--window-bg)",
						color: "var(--window-fg)",
					}}
				>
					{value || (error ?? "—")}
				</output>
				<button
					type="button"
					onClick={() => void regenerate()}
					className="cursor-pointer rounded-[6px] px-2 py-1"
					style={{ color: "var(--window-active-text-fg)" }}
				>
					Again
				</button>
			</div>

			<div className="mb-2 flex items-center gap-2">
				<label
					htmlFor="gen-length"
					className="text-[var(--window-sub-text-fg)]"
				>
					Length
				</label>
				<input
					id="gen-length"
					type="range"
					min={8}
					max={64}
					value={length}
					onChange={(e) => setLength(Number(e.target.value))}
					className="flex-1"
					style={{ accentColor: "var(--window-bg-active)" }}
				/>
				<span className="w-6 text-right tabular-nums">{length}</span>
			</div>

			<div className="mb-3 flex flex-wrap gap-x-3 gap-y-1">
				<Check
					label="a–z"
					on={options.lowercase}
					onChange={() => toggle("lowercase")}
				/>
				<Check
					label="A–Z"
					on={options.uppercase}
					onChange={() => toggle("uppercase")}
				/>
				<Check
					label="0–9"
					on={options.digits}
					onChange={() => toggle("digits")}
				/>
				<Check
					label="!@#"
					on={options.symbols}
					onChange={() => toggle("symbols")}
				/>
				<Check
					label="No look-alikes"
					on={options.excludeAmbiguous}
					onChange={() => toggle("excludeAmbiguous")}
				/>
			</div>

			<div className="flex gap-2">
				<button
					type="button"
					disabled={!value}
					onClick={() => onUse(value)}
					className="cursor-pointer rounded-[6px] px-3 py-1.5 font-semibold disabled:opacity-45"
					style={{
						background: "var(--active-button-bg)",
						color: "var(--window-fg-active)",
					}}
				>
					Use
				</button>
				<button
					type="button"
					onClick={onClose}
					className="cursor-pointer rounded-[6px] px-3 py-1.5"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					Cancel
				</button>
			</div>
		</div>
	);
}

function Check({
	label,
	on,
	onChange,
}: {
	label: string;
	on: boolean;
	onChange: () => void;
}) {
	return (
		<label className="flex cursor-pointer items-center gap-1.5">
			<input
				type="checkbox"
				checked={on}
				onChange={onChange}
				style={{ accentColor: "var(--window-bg-active)" }}
			/>
			<span>{label}</span>
		</label>
	);
}
