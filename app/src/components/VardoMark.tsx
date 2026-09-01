/**
 * Vardo's mark.
 *
 * A vardo is a Romani wagon: a home you carry with you, which is what a vault
 * that follows you between machines is. So the mark is the wagon's end-on
 * silhouette - the barrel-vaulted roof, the eaves it overhangs by, and a door -
 * rather than the padlock every password manager reaches for.
 *
 * The padlock it replaces was also sitting in a filled circle, which is the
 * shape you use when you have a generic glyph and need it to look deliberate.
 * A mark should not need a bubble to hold it up.
 *
 * Drawn against four alternatives and checked at 16px, because a logo that only
 * works at 64 is a picture rather than a mark. The ones with a wheel read as a
 * handbag; the one with a keyhole read as a padlock again by another route; the
 * one without a door read as a headstone. The eaves are what stop this being a
 * generic arch, and they are the part that survives being made small.
 */

interface Props {
	size?: number;
	/** Defaults to the accent. Pass a colour to match surrounding text. */
	color?: string;
}

export function VardoMark({ size = 64, color = "var(--window-bg-active)" }: Props) {
	return (
		<svg
			width={size}
			height={size}
			viewBox="0 0 120 120"
			role="img"
			aria-label="Vardo"
			style={{ display: "block" }}
		>
			<title>Vardo</title>
			{/* One path, even-odd, so the doorway is a hole rather than a shape
			    painted in the background colour. Painting it would break the
			    moment the mark sat on anything but the exact background it was
			    drawn against. */}
			<path
				fill={color}
				fillRule="evenodd"
				d="
          M 18 100 L 18 46 A 42 42 0 0 1 102 46 L 102 100 Z
          M 46 100 L 46 74 A 14 14 0 0 1 74 74 L 74 100 Z
        "
			/>
			{/* The eaves. A separate rounded bar rather than part of the outline,
			    because the overhang is the detail that says wagon and it needs to
			    stay crisp when the whole thing is 16 pixels wide. */}
			<rect x="4" y="38" width="112" height="11" rx="5.5" fill={color} />
		</svg>
	);
}
