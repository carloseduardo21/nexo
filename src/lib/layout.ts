// Árvore de divisão de telas. Funções puras que preservam a identidade dos nós
// não alterados, para o Svelte só remontar o que realmente mudou.

export type SplitDir = 'row' | 'col';

export type PaneNode =
  | { type: 'leaf'; id: string }
  | { type: 'split'; id: string; dir: SplitDir; ratio: number; a: PaneNode; b: PaneNode };

export type SplitNode = Extract<PaneNode, { type: 'split' }>;

let splitSeq = 0;

export function splitLeaf(node: PaneNode, target: string, dir: SplitDir, newId: string): PaneNode {
  if (node.type === 'leaf') {
    if (node.id !== target) return node;
    return { type: 'split', id: `s${++splitSeq}`, dir, ratio: 0.5, a: node, b: { type: 'leaf', id: newId } };
  }
  const a = splitLeaf(node.a, target, dir, newId);
  const b = splitLeaf(node.b, target, dir, newId);
  return a === node.a && b === node.b ? node : { ...node, a, b };
}

/** Remove uma folha; o irmão ocupa o lugar do split. Retorna null se a árvore ficou vazia. */
export function removeLeaf(node: PaneNode, target: string): PaneNode | null {
  if (node.type === 'leaf') return node.id === target ? null : node;
  const a = removeLeaf(node.a, target);
  const b = removeLeaf(node.b, target);
  if (!a) return b;
  if (!b) return a;
  return a === node.a && b === node.b ? node : { ...node, a, b };
}

export function leaves(node: PaneNode, out: string[] = []): string[] {
  if (node.type === 'leaf') out.push(node.id);
  else {
    leaves(node.a, out);
    leaves(node.b, out);
  }
  return out;
}
