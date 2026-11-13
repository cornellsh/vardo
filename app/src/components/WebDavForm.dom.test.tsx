import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebDavForm } from "./WebDavForm";

afterEach(cleanup);

function fill(url: string, password: string) {
	fireEvent.change(screen.getByLabelText("Address of the vault file"), { target: { value: url } });
	fireEvent.change(screen.getByLabelText("Username"), { target: { value: "alex" } });
	fireEvent.change(screen.getByLabelText("Server password"), { target: { value: password } });
	fireEvent.click(screen.getByRole("button", { name: "Connect" }));
}

describe("WebDavForm", () => {
	it("hands back the vault once the server accepts the credentials", async () => {
		const onConnected = vi.fn();
		render(<WebDavForm onConnected={onConnected} />);
		fill("https://cloud.example.com/dav/passwords.kdbx", "dav");
		await waitFor(() =>
			expect(onConnected).toHaveBeenCalledWith({
				kind: "webDav",
				url: "https://cloud.example.com/dav/passwords.kdbx",
				username: "alex",
			}),
		);
	});

	it("shows the refusal beside the form and hands back nothing", async () => {
		const onConnected = vi.fn();
		render(<WebDavForm onConnected={onConnected} />);
		fill("https://cloud.example.com/dav/passwords.kdbx", "wrong");
		await screen.findByText(/refused these credentials/);
		expect(onConnected).not.toHaveBeenCalled();
	});
});
