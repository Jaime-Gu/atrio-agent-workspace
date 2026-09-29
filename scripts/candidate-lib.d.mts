export interface BuildIdentity {
  candidateId: string | null;
  buildId: string;
  sourceFingerprint: string | null;
}
export function buildIdentity(root: string, channel: string): BuildIdentity;
export function viteBuildContext(channel: string): {
  identity: BuildIdentity;
  webOutDir: string | undefined;
};
