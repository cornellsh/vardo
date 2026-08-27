import driveLogo from "../assets/google-drive.png";

/**
 * The Google Drive mark.
 *
 * This is Google's own artwork, not a reconstruction. I first tried rebuilding
 * it as an SVG from measurements, and it was not good enough: the real mark's
 * character is in soft gradient blends along each seam, and a hand-built
 * version had hard edges and read as a knock-off. A slightly-wrong trademark is
 * worse than none.
 *
 * The asset is the May 2026 redesign, which matters because the icon changed
 * recently and the version most people picture is the 2020 one. The current
 * mark dropped red entirely, has a rounded outer triangle, and keeps a sharp
 * inverted triangle of negative space at the centre. Notably `ssl.gstatic.com`
 * still serves the *old* design, so the obvious source is the wrong one; this
 * came from the current listing artwork, with white made transparent so it sits
 * on either theme.
 *
 * On trademark: it identifies Google Drive on a control that connects to Google
 * Drive, which is what a mark is for. It is unmodified apart from the
 * transparency, is never used as Vardo's own branding, and always appears
 * beside the words "Google Drive".
 */

interface Props {
	size?: number;
	/** Desaturated, for a not-yet-connected state. */
	muted?: boolean;
}

export function DriveLogo({ size = 24, muted = false }: Props) {
	return (
		<img
			src={driveLogo}
			alt="Google Drive"
			width={size}
			height={size}
			style={{
				display: "block",
				// The source is 192x178, so height is the constraining side; letting
				// it letterbox rather than stretching keeps the geometry honest.
				width: size,
				height: "auto",
				filter: muted ? "grayscale(1)" : undefined,
				opacity: muted ? 0.5 : 1,
				transition:
					"opacity var(--duration-universal) linear, filter var(--duration-universal) linear",
			}}
		/>
	);
}
