import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { BrowserConnect } from "./BrowserConnect";

afterEach(cleanup);

describe("BrowserConnect", () => {
	it("connects every browser that has run, then disconnects them", async () => {
		render(<BrowserConnect />);
		fireEvent.click(await screen.findByRole("button", { name: "Connect" }));
		await screen.findByText("Connected to Google Chrome, Brave");
		fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
		await screen.findByText("Fill passwords in Chrome, Brave, Edge and others");
		expect(screen.getByRole("button", { name: "Connect" })).toBeTruthy();
	});
});
