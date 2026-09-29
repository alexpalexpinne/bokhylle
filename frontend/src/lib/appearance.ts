export type ShelfFinish = 'oak' | 'black' | 'metal'

export function shelfFinish(value?: string): ShelfFinish {
  return value === 'black' || value === 'metal' ? value : 'oak'
}
