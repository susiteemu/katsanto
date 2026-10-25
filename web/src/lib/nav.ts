/** Where to go after logging in: `?next=` if it's a path on this site, else home. */
export function nextPath(url: { searchParams: { get(name: string): string | null } }): string {
	const next = url.searchParams.get('next');
	// Reject `//host` and `/\\host` (protocol-relative) and anything not starting with `/`.
	return next && /^\/(?![/\\])/.test(next) ? next : '/';
}
