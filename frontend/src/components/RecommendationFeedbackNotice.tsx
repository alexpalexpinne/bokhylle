import { undoRecommendationFeedback } from '../api/recommendations'
import { ActionNotice } from './ui/ActionNotice'

export type RecommendationNotice = { key: string; undoToken: string; title: string; action: 'like' | 'not_for_me' | 'dismiss' }

export function RecommendationFeedbackNotice({ notice, onDismiss }: { notice: RecommendationNotice; onDismiss: () => void }) {
  const message = notice.action === 'like' ? `Liked “${notice.title}”.`
    : notice.action === 'not_for_me' ? `“${notice.title}” is marked not for me.`
    : `Set aside “${notice.title}” for seven days.`
  return <ActionNotice key={notice.undoToken} message={message} onDismiss={onDismiss} onUndo={() => undoRecommendationFeedback(notice.key, notice.undoToken)} />
}
