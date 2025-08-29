## GhostPost MVP - Complete PRD for Development

**Product:** Mobile app for younger Gen Z users to audit and clean social media history before college applications, job applications, or greek life rushing.

**Platform:** Expo (React Native)

## Complete User Flow & Screens

### 1. Splash Screen
```
Components:
- Animated ghost mascot (SVG/Lottie animation)
- Gradient background (purple #6B46C1 to blue #2563EB)
- App logo at top
- Tagline: "Your past doesn't have to haunt you"
- Subtext: "Clean up your socials before they matter"
- CTA Button: "Get Started" (white text, dark button, bounce animation)
```

### 2. Concern Selection Screen
```
Components:
- Header: "What's coming up?"
- Multi-select grid (2 columns):
  - Each card: Icon + Label + Selection state
  - Selected state: Glow effect + checkmark
- Options with specific icons:
  - 🎓 College Applications
  - 💼 Internship Hunt
  - 🏫 Greek Life Rush
  - 💕 New Relationship
  - 🎯 Athletic Recruitment
  - 👔 First Real Job
- Ghost reaction changes based on selection
- "Next" button (disabled until 1+ selected)
```

### 3. Platform Selection Screen
```
Components:
- Header: "Where have you been posting?"
- Platform cards (vertical scroll):
  - Instagram
  - TikTok
  - Twitter/X
  - Facebook
- Multi-select with checkbox
- "Start Scanning" button (disabled until 1+ selected)
```

### 4. Dashboard Screen
```
Components:
- Overall Progress Ring:
  - Circular progress indicator
  - Center text: "73% Safe"
  - Subtitle: "28 posts need review"
  
- Platform Cards (horizontal scroll):
  Card structure:
  - Platform icon
  - Platform name
  - Progress bar
  - Status text:
    - "Not connected" (gray)
    - "Upload needed" (yellow)
    - "Processing..." (blue)
    - "12 flagged" (red)
    - "All clear!" (green)
  - Tap action based on status

- CTA Button:
  - Primary: "Review Flagged Posts" (if any flagged)
  - Secondary: "Connect Next Platform" (if none flagged)

- Bottom Navigation:
  - Dashboard (home icon)
  - Review (swipe icon)
  - Settings (gear icon)
```

### 5. Data Upload Screen (Per Platform)
```
Components:
- Back button
- Platform logo
- Step indicator (1 of 3, 2 of 3, etc.)

Step 1 - Explanation:
- Friendly text about privacy
- "Your data stays on your device"
- "Continue" button

Step 2 - Tutorial:
- GIF/Video showing exact steps
- Platform-specific instructions:
  - Instagram: Settings > Privacy > Download Data
  - Twitter: Settings > Account > Download Data
  - TikTok: Settings > Privacy > Download Data
  - Facebook: Settings > Your Information > Download
- "I've requested my data" button

Step 3 - Upload:
- Drag/drop zone (desktop) or file picker (mobile)
- Accepted formats per platform:
  - Instagram: .zip file
  - Twitter: .zip file
  - TikTok: .json or .txt files
  - Facebook: .zip file
- Upload progress indicator
- Error handling for wrong format
```

### 6. Review Screen (Swipe Interface)
```
Components:
- Card stack showing posts
- Post card contains:
  - Platform icon
  - Date posted + your age then
  - Post content (text/image thumbnail)
  - Engagement metrics (likes, comments)
  - Risk indicator (High/Medium/Low)
  - Why flagged: Brief explanation

- Swipe gestures:
  - Left (Keep): Green overlay
  - Right (Delete): Red overlay
  - Up (Unsure): Yellow overlay

- Bottom buttons (alternative to swipe):
  - Keep (green)
  - Delete (red)
  - Skip (gray)

- Progress indicator: "12 of 47 reviewed"
- Undo button (last action only)
```

### 7. Results/Action Screen
```
Components:
- Summary stats:
  - Total reviewed
  - Marked for deletion
  - Kept
  - Unsure

- Export options:
  - "Get deletion checklist" (PDF)
  - "Share results" (image)

- Platform-grouped deletion list:
  - Direct links to posts
  - Batch selection
  - "Open in app" buttons
```

## Data Processing Logic

### Risk Detection Categories & Keywords

```javascript
const riskCategories = {
  substances: {
    keywords: ['drunk', 'wasted', 'high', 'smoke', 'vape', '420', ...],
    severity: 'high',
    context_check: true
  },
  profanity: {
    keywords: [/* comprehensive list */],
    severity: 'medium',
    context_check: true
  },
  controversial_topics: {
    keywords: ['politics', 'religion', ...],
    severity: 'low',
    context_check: true
  },
  sexual_content: {
    keywords: [/* appropriate list */],
    severity: 'high',
    context_check: true
  },
  negative_behavior: {
    keywords: ['hate', 'fight', 'cancel', ...],
    severity: 'medium',
    context_check: true
  }
}
```

### Platform Data Parsers

```javascript
// Instagram parser
const parseInstagram = (zipFile) => {
  // Extract: content/posts_1.json
  // Extract: comments/comments.json
  // Return standardized format
}

// Twitter parser
const parseTwitter = (zipFile) => {
  // Extract: data/tweets.js
  // Extract: data/direct-messages.js
  // Return standardized format
}

// TikTok parser
const parseTikTok = (files) => {
  // Parse: Video List.txt
  // Parse: Comment/comment.json
  // Return standardized format
}

// Facebook parser
const parseFacebook = (zipFile) => {
  // Extract: posts/your_posts_1.json
  // Extract: comments/comments.json
  // Return standardized format
}

// Standardized post format
{
  id: string,
  platform: 'instagram' | 'twitter' | 'tiktok' | 'facebook',
  type: 'post' | 'comment' | 'reply',
  content: string,
  date: Date,
  engagement: { likes: number, comments: number },
  mediaUrl?: string,
  permalink: string,
  riskScore: number (0-100),
  flaggedReasons: string[]
}
```

## Animation Specifications

### Ghost Mascot States
1. **Idle**: Gentle floating bob (2s loop)
2. **Happy**: Jump with smile
3. **Worried**: Side-to-side shake
4. **Scanning**: Magnifying glass animation
5. **Celebrating**: Confetti burst

### Transitions
- Screen transitions: 300ms slide
- Card swipes: Spring physics
- Progress bars: Ease-in-out
- Button presses: Scale 0.95 with haptic

## Error Handling

```javascript
// File upload errors
- Wrong format: "This doesn't look like a [Platform] export"
- Too large: "File too big - try splitting your export"
- Parsing failed: "Something went wrong - try downloading again"

// Network errors (for opening links)
- No connection: "Check your internet to open in [Platform]"

// Processing errors
- Crash recovery: Save progress every 10 posts
- Resume capability: "Continue where you left off?"
```

## Settings Screen

```
- Notification preferences
- Privacy policy link
- Terms of service link
- About/Help
- Export all decisions
```

## Copy/Microcopy Document

```
Empty states:
- No platforms: "Connect your first platform to start"
- No flags: "You're all clear! 🎉"
- Processing: "Ghost is scanning your posts..."

Error messages:
- Upload failed: "Oops! Let's try that again"
- Parse error: "This file seems corrupted"

Success messages:
- Upload complete: "Got it! Let's check your posts"
- Review complete: "You've reviewed everything!"
```
