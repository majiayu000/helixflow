export type CreationAsset = {
  id: string;
  title: string;
  kind: 'image' | 'video' | 'audio';
  createdAt: number;
  type: string;
};

const DB_NAME = 'helixflow-creation';
const STORE = 'assets';

export async function listCreationAssets(): Promise<CreationAsset[]> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const request = db.transaction(STORE, 'readonly').objectStore(STORE).getAll();
    request.onerror = () => reject(request.error ?? new Error('读取素材库失败'));
    request.onsuccess = () => {
      const rows = (request.result as Array<CreationAsset & { blob?: Blob }>)
        .map(({ blob: _blob, ...asset }) => asset)
        .sort((left, right) => right.createdAt - left.createdAt);
      resolve(rows);
    };
  });
}

export async function addCreationAsset(input: {
  title: string;
  kind: CreationAsset['kind'];
  file: Blob;
}): Promise<CreationAsset> {
  const db = await openDb();
  const asset: CreationAsset & { blob: Blob } = {
    id: `asset_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 6)}`,
    title: input.title,
    kind: input.kind,
    createdAt: Date.now(),
    type: input.file.type || 'application/octet-stream',
    blob: input.file,
  };
  return new Promise((resolve, reject) => {
    const request = db.transaction(STORE, 'readwrite').objectStore(STORE).put(asset);
    request.onerror = () => reject(request.error ?? new Error('保存素材失败'));
    request.onsuccess = () => resolve({
      id: asset.id,
      title: asset.title,
      kind: asset.kind,
      createdAt: asset.createdAt,
      type: asset.type,
    });
  });
}

export async function readCreationAssetFile(id: string): Promise<File> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const request = db.transaction(STORE, 'readonly').objectStore(STORE).get(id);
    request.onerror = () => reject(request.error ?? new Error('读取素材失败'));
    request.onsuccess = () => {
      const row = request.result as { title?: string; type?: string; blob?: Blob } | undefined;
      if (!row?.blob) {
        reject(new Error('素材不存在'));
        return;
      }
      resolve(new File([row.blob], row.title || id, { type: row.type || row.blob.type }));
    };
  });
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    if (typeof indexedDB === 'undefined') {
      reject(new Error('这个浏览器没有 IndexedDB'));
      return;
    }
    const request = indexedDB.open(DB_NAME, 1);
    request.onerror = () => reject(request.error ?? new Error('打开素材库失败'));
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(STORE)) {
        request.result.createObjectStore(STORE, { keyPath: 'id' });
      }
    };
    request.onsuccess = () => resolve(request.result);
  });
}
