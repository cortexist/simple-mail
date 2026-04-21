export const networkActivity = $state({ count: 0 });

export function incNetwork() {
  networkActivity.count++;
}

export function decNetwork() {
  if (networkActivity.count > 0) networkActivity.count--;
}
