import type { GroupNode } from "../ipc";

/** The group with this UUID anywhere in the tree. */
export function findGroup(root: GroupNode, uuid: string): GroupNode | undefined {
	if (root.uuid === uuid) return root;
	for (const child of root.children) {
		const found = findGroup(child, uuid);
		if (found) return found;
	}
	return undefined;
}

/**
 * Finds the one group a typed name means, for moving a group without dragging.
 *
 * Case-insensitive. An empty name means the top level. A name that matches no
 * group, or several, is an error in words, since guessing would file a group
 * somewhere the person did not pick.
 */
export function groupByName(root: GroupNode, typed: string): GroupNode | string {
	const name = typed.trim();
	if (!name) return root;
	const matches: GroupNode[] = [];
	const walk = (g: GroupNode) => {
		if (g !== root && g.name.toLowerCase() === name.toLowerCase()) matches.push(g);
		g.children.forEach(walk);
	};
	walk(root);
	if (matches.length === 1) return matches[0] as GroupNode;
	return matches.length === 0
		? `No group is called ${name}.`
		: `More than one group is called ${name}. Drag it there instead.`;
}
