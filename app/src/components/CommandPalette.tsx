import { useState } from "react";

export interface Command {
	id: string;
	label: string;
	/** Shown beside the label, so the palette also teaches the shortcut. */
	shortcut?: string;
	run: () => void;
}

interface Props {
	commands: Command[];
	onClose: () => void;
}

/** Every word typed must appear in the label, in any order. */
function matches(label: string, query: string): boolean {
	const text = label.toLowerCase();
	return query
		.toLowerCase()
		.split(/\s+/)
		.filter(Boolean)
		.every((word) => text.includes(word));
}

/**
 * Every action in the window, one search away (Ctrl+K).
 *
 * It holds no logic of its own: each command is a callback the vault screen
 * already uses for its buttons and shortcuts, so the palette cannot drift from
 * what the buttons do.
 */
export function CommandPalette({ commands, onClose }: Props) {
	const [query, setQuery] = useState("");
	const [selected, setSelected] = useState(0);
	const shown = commands.filter((c) => matches(c.label, query));

	function run(command: Command) {
		onClose();
		command.run();
	}

	function onKeyDown(e: React.KeyboardEvent) {
		if (e.key === "Escape") {
			// Stops here, so Escape closes the palette and not the search or the
			// drawer behind it as well.
			e.preventDefault();
			e.stopPropagation();
			onClose();
		} else if (e.key === "ArrowDown") {
			e.preventDefault();
			setSelected((i) => Math.min(i + 1, shown.length - 1));
		} else if (e.key === "ArrowUp") {
			e.preventDefault();
			setSelected((i) => Math.max(i - 1, 0));
		} else if (e.key === "Enter") {
			e.preventDefault();
			const command = shown[selected];
			if (command) run(command);
		}
	}

	return (
		<div className="fixed inset-0 z-50 flex items-start justify-center pt-[12vh]">
			<button
				type="button"
				aria-label="Close commands"
				tabIndex={-1}
				onClick={onClose}
				className="absolute inset-0 cursor-default"
				style={{ background: "rgba(0, 0, 0, 0.35)" }}
			/>
			<div
				role="dialog"
				aria-modal="true"
				aria-label="Commands"
				className="relative w-[min(520px,calc(100vw-32px))] overflow-hidden rounded-[8px]"
				style={{
					background: "var(--window-bg)",
					border: "1px solid var(--shadow-fg)",
					boxShadow: "0 12px 40px rgba(0, 0, 0, 0.35)",
				}}
			>
				<input
					// The palette opens on a key press, so typing must go here at once.
					// eslint-disable-next-line jsx-a11y/no-autofocus
					autoFocus
					value={query}
					onChange={(e) => {
						setQuery(e.target.value);
						setSelected(0);
					}}
					onKeyDown={onKeyDown}
					placeholder="Type a command"
					aria-label="Type a command"
					aria-controls="command-list"
					autoComplete="off"
					spellCheck={false}
					className="w-full px-4 py-3 text-[15px] outline-none"
					style={{ background: "var(--window-bg)", borderBottom: "1px solid var(--shadow-fg)" }}
				/>
				<div id="command-list" role="listbox" aria-label="Commands" className="max-h-[50vh] overflow-y-auto py-1">
					{shown.map((command, i) => (
						<button
							key={command.id}
							type="button"
							role="option"
							aria-selected={i === selected}
							tabIndex={-1}
							onMouseEnter={() => setSelected(i)}
							onClick={() => run(command)}
							className="flex w-full items-center justify-between px-4 py-2 text-left"
							style={{
								background: i === selected ? "var(--dialogs-bg-active)" : "transparent",
								color: i === selected ? "var(--window-fg-active)" : "var(--window-bold-fg)",
							}}
						>
							<span>{command.label}</span>
							{command.shortcut && <kbd className="text-[12px] opacity-80">{command.shortcut}</kbd>}
						</button>
					))}
					{shown.length === 0 && (
						<p className="px-4 py-3 text-[var(--window-sub-text-fg)]">No command matches.</p>
					)}
				</div>
			</div>
		</div>
	);
}
