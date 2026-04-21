import type { Email, Folder, CalendarEvent, CalendarCategory, FullContact, Account } from '$lib/types';

// ── Date helper ──────────────────────────────────────
// All mock dates are relative to "today" so the demo always looks fresh.

/** Return a Date for today ± dayOffset at the given hour:minute. */
function d(dayOffset: number, hour = 0, minute = 0): Date {
  const date = new Date();
  date.setDate(date.getDate() + dayOffset);
  date.setHours(hour, minute, 0, 0);
  return date;
}

/** Return a Date for today ± dayOffset, all-day start (midnight). */
function dAllDay(dayOffset: number): Date {
  return d(dayOffset, 0, 0);
}

/** Return a Date for today ± dayOffset, all-day end (23:59:59). */
function dAllDayEnd(dayOffset: number): Date {
  const date = d(dayOffset, 23, 59);
  date.setSeconds(59);
  return date;
}

// ── Priority-bucket heuristic (mock only) ──────────────

function withMockFocusedFlag(emails: Email[]): Email[] {
  return emails.map((email) => {
    if (typeof email.isFocused === 'boolean') return email;

    // Mock-only heuristic until backend classification is implemented.
    if (email.folder !== 'inbox') {
      return { ...email, isFocused: true };
    }

    const sender = `${email.from.name} ${email.from.email}`.toLowerCase();
    const subject = email.subject.toLowerCase();
    const isLikelyBulk =
      sender.includes('noreply') ||
      sender.includes('newsletter') ||
      sender.includes('deals') ||
      sender.includes('offers') ||
      subject.includes('invitation') ||
      subject.includes('policy');

    return { ...email, isFocused: !isLikelyBulk };
  });
}

/* ═══════════════════════════════════════════════════════
   ACCOUNT 1 — Work (Cortexist Inc.)
   ═══════════════════════════════════════════════════════ */

const workFolders: Folder[] = [
  { id: 'inbox',   name: 'Inbox',         icon: 'inbox',   isFavorite: true  },
  { id: 'sent',    name: 'Sent Items',    icon: 'sent',    isFavorite: true  },
  { id: 'drafts',  name: 'Drafts',        icon: 'drafts',  isFavorite: true  },
  { id: 'deleted', name: 'Deleted Items', icon: 'deleted', isFavorite: false },
  { id: 'junk',    name: 'Junk Email',    icon: 'junk',    isFavorite: false },
  { id: 'archive', name: 'Archive',       icon: 'archive', isFavorite: false },
];

const workEmails: Email[] = [
  {
    id: '1',
    from: { name: 'Sara Chen', email: 'sara.chen@company.com', initials: 'SC', color: '#0078d4' },
    to: [{ name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' }],
    subject: 'Product Roadmap Review',
    preview: 'Hi team, I wanted to share the updated roadmap for your review. Please take a look at the attached document and let me know if you have any questions…',
    body: `<p>Hi team,</p>
<p>I wanted to share the updated roadmap for your review. Please take a look at the attached document and let me know if you have any questions or concerns.</p>
<p><strong>Key highlights:</strong></p>
<ul>
  <li>New mail client launch — next month</li>
  <li>Calendar integration — following month</li>
  <li>Mobile app beta — end of quarter</li>
</ul>
<p>Let's discuss in our Thursday meeting.</p>
<p>Best,<br/>Sara</p>`,
    date: d(0, 9, 30),
    isRead: false,
    isStarred: true,
    isPinned: false,
    hasAttachment: true,
    folder: 'inbox',
  },
  {
    id: '2',
    from: { name: 'Michael Torres', email: 'michael.t@company.com', initials: 'MT', color: '#498205' },
    to: [{ name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' }],
    subject: 'Team Lunch Friday 🍕',
    preview: "Hey everyone! I'm organizing a team lunch this Friday at noon. We're thinking Italian — does that work for everyone?",
    body: `<p>Hey everyone!</p>
<p>I'm organizing a team lunch this Friday at noon. We're thinking Italian — does that work for everyone?</p>
<p><strong>Options:</strong></p>
<ol>
  <li>Olive Garden (casual)</li>
  <li>Carrabba's (slightly nicer)</li>
  <li>Local pizza place (quick and easy)</li>
</ol>
<p>Reply with your preference by Wednesday!</p>
<p>Cheers,<br/>Michael</p>`,
    date: d(0, 8, 15),
    isRead: false,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  {
    id: '3',
    from: { name: 'Emily Wang', email: 'emily.wang@company.com', initials: 'EW', color: '#8764b8' },
    to: [{ name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' }],
    subject: 'Design System v3.0 Update',
    preview: "The new design system is ready for review. I've updated all components to use the new Fluent tokens and added dark mode support…",
    body: `<p>Hi all,</p>
<p>The new design system is ready for review. I've updated all components to use the new Fluent Design tokens and added comprehensive dark mode support.</p>
<p><strong>Changes include:</strong></p>
<ul>
  <li>Updated color tokens to match Windows 11 Fluent Design</li>
  <li>New spacing and typography scales</li>
  <li>Dark mode for all 47 components</li>
  <li>Improved accessibility (WCAG 2.1 AA compliance)</li>
</ul>
<p>Please review the Figma file linked in the project board and leave your feedback by EOW.</p>
<p>Thanks,<br/>Emily</p>`,
    date: d(-1, 16, 45),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: true,
    folder: 'inbox',
  },
  {
    id: '4',
    from: { name: 'Roku Kaneshiro', email: 'roku.kaneshiro@company.com', initials: 'DK', color: '#da3b01' },
    to: [{ name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' }],
    subject: 'Re: Budget Approval for Q2',
    preview: 'Good news — the budget has been approved! We can proceed with the hiring plan and infrastructure upgrades as discussed…',
    body: `<p>Good news!</p>
<p>The budget has been approved! We can proceed with the hiring plan and infrastructure upgrades as discussed in last week's meeting.</p>
<p><strong>Approved items:</strong></p>
<ul>
  <li>3 new engineering hires — $450K</li>
  <li>Cloud infrastructure upgrade — $120K</li>
  <li>Design tooling licenses — $30K</li>
  <li>Team offsite Q2 — $25K</li>
</ul>
<p>I'll set up individual meetings to discuss timelines for each item.</p>
<p>Best,<br/>Takeshi</p>`,
    date: d(-1, 14, 20),
    isRead: true,
    isStarred: true,
    isPinned: false,
    hasAttachment: true,
    folder: 'inbox',
  },
  {
    id: '5',
    from: { name: 'Alex Johnson', email: 'alex.j@company.com', initials: 'AJ', color: '#00b7c3' },
    to: [{ name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' }],
    subject: 'Weekly Standup Notes',
    preview: "Here are the notes from this week's standup. Action items are highlighted in bold. Please check your assignments…",
    body: `<p>Team,</p>
<p>Here are the notes from this week's standup:</p>
<p><strong>Completed:</strong></p>
<ul>
  <li>Auth service migration to v2 — Alex</li>
  <li>UI component library audit — Emily</li>
  <li>Performance benchmarks — Sara</li>
</ul>
<p><strong>In Progress:</strong></p>
<ul>
  <li>Mail client frontend — You</li>
  <li>Calendar API integration — Michael</li>
  <li>Database optimization — Takeshi</li>
</ul>
<p><strong>Blockers:</strong></p>
<ul>
  <li>Need DevOps review for deployment pipeline changes</li>
</ul>
<p>Next standup: Monday at 10 AM.</p>
<p>— Alex</p>`,
    date: d(-2, 10, 0),
    isRead: false,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  {
    id: '6',
    from: { name: 'HR Department', email: 'hr@company.com', initials: 'HR', color: '#c239b3' },
    to: [{ name: 'All Staff', email: 'all@company.com', initials: 'AS', color: '#c239b3' }],
    subject: 'Updated Remote Work Policy',
    preview: 'Please review the updated remote work policy that takes effect next month. Key changes include flexible core hours…',
    body: `<p>Dear Team,</p>
<p>Please review the updated remote work policy that takes effect next month.</p>
<p><strong>Key changes:</strong></p>
<ul>
  <li><strong>Flexible core hours:</strong> 10 AM – 3 PM (previously 9 AM – 4 PM)</li>
  <li><strong>Remote days:</strong> Up to 3 days per week (previously 2)</li>
  <li><strong>Home office stipend:</strong> Increased to $1,000/year</li>
  <li><strong>Quarterly in-person meetings:</strong> Required for all teams</li>
</ul>
<p>Please acknowledge receipt of this policy through the HR portal.</p>
<p>Questions? Reach out to your HR business partner.</p>
<p>Best regards,<br/>Human Resources</p>`,
    date: d(-3, 9, 0),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: true,
    folder: 'inbox',
  },
  {
    id: '7',
    from: { name: 'Lisa Bergström', email: 'lisa.bergström@company.com', initials: 'LP', color: '#e3008c' },
    to: [{ name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' }],
    subject: 'Project Phoenix Kickoff',
    preview: 'Excited to announce the kickoff for Project Phoenix! This will be our flagship product for this year. Meeting details inside…',
    body: `<p>Hi everyone,</p>
<p>Excited to announce the kickoff for <strong>Project Phoenix</strong>! This will be our flagship product initiative.</p>
<p><strong>Kickoff Meeting:</strong></p>
<ul>
  <li>Date: Next week</li>
  <li>Time: 2:00 PM – 4:00 PM</li>
  <li>Location: Conference Room A / Teams Link</li>
</ul>
<p><strong>Agenda:</strong></p>
<ol>
  <li>Project vision and goals (30 min)</li>
  <li>Technical architecture overview (30 min)</li>
  <li>Team structure and roles (20 min)</li>
  <li>Timeline and milestones (20 min)</li>
  <li>Q&A (20 min)</li>
</ol>
<p>Please come prepared with questions. Pre-reading materials are attached.</p>
<p>— Lisa</p>`,
    date: d(-4, 11, 30),
    isRead: true,
    isStarred: true,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  {
    id: '8',
    from: { name: 'James Wilson', email: 'james.w@partner.com', initials: 'JW', color: '#986f0b' },
    to: [{ name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' }],
    subject: 'Invitation: Product Demo',
    preview: "You're invited to an exclusive demo of our new collaboration platform. We think it would be a great fit for your team…",
    body: `<p>Hello,</p>
<p>You're invited to an exclusive demo of our new collaboration platform, <strong>SyncSpace</strong>.</p>
<p>Based on your team's needs, we think SyncSpace would be a great fit for:</p>
<ul>
  <li>Real-time document collaboration</li>
  <li>Integrated project management</li>
  <li>AI-powered meeting summaries</li>
  <li>Cross-platform support (Windows, macOS, Linux, mobile)</li>
</ul>
<p><strong>Demo Details:</strong></p>
<ul>
  <li>Date: Next week</li>
  <li>Time: 11:00 AM EST</li>
  <li>Duration: 45 minutes</li>
</ul>
<p>Looking forward to showing you what we've built!</p>
<p>Best,<br/>James Wilson<br/>Partner Relations, SyncSpace</p>`,
    date: d(-7, 15, 0),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  /* ── Drafts ── */
  {
    id: '9',
    from: { name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' },
    to: [{ name: 'Team', email: 'team@company.com', initials: 'TM', color: '#498205' }],
    subject: 'Quarterly Report Draft',
    preview: "Here's the draft of our quarterly report. Please review sections 2 and 3 which cover our product milestones…",
    body: `<p>Hi Team,</p>
<p>Here's the draft of our quarterly report. Please review sections 2 and 3 which cover our product milestones and financial projections.</p>
<p><em>[Draft in progress…]</em></p>`,
    date: d(-1, 17, 0),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'drafts',
    draftData: {
      to: 'Team <team@company.com>',
      cc: '',
      bcc: '',
      subject: 'Quarterly Report Draft',
      body: `<p>Hi Team,</p>
<p>Here's the draft of our quarterly report. Please review sections 2 and 3 which cover our product milestones and financial projections.</p>
<p><em>[Draft in progress…]</em></p>`,
      showCc: false,
      showBcc: false,
      attachments: [],
    },
  },
  /* ── Sent ── */
  {
    id: '10',
    from: { name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' },
    to: [{ name: 'Sara Chen', email: 'sara.chen@company.com', initials: 'SC', color: '#0078d4' }],
    subject: 'Re: Product Roadmap Review',
    preview: "Thanks Sara! I've reviewed the roadmap and everything looks great. Just a few suggestions on the timeline for the mobile…",
    body: `<p>Thanks Sara!</p>
<p>I've reviewed the roadmap and everything looks great. Just a few suggestions on the timeline for the mobile app beta — I think we might need an extra two weeks given the complexity of offline sync.</p>
<p>Let's discuss Thursday.</p>
<p>Best</p>`,
    date: d(0, 10, 0),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'sent',
  },
  {
    id: '11',
    from: { name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' },
    to: [{ name: 'Michael Torres', email: 'michael.t@company.com', initials: 'MT', color: '#498205' }],
    subject: 'Re: Team Lunch Friday',
    preview: "Count me in! I'd vote for the local pizza place — quick and easy works best for me this week.",
    body: `<p>Count me in! I'd vote for the local pizza place — quick and easy works best for me this week.</p>
<p>See you Friday!</p>`,
    date: d(0, 8, 45),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'sent',
  },
  /* ── Junk ── */
  {
    id: '12',
    from: { name: 'Prize Center', email: 'noreply@prizes-winner.biz', initials: 'PC', color: '#8a8a8a' },
    to: [{ name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' }],
    subject: 'Congratulations! You have won!',
    preview: 'You have been selected as the winner of our exclusive giveaway. Click here to claim your $10,000 prize now before it expires…',
    body: `<p>🎉 CONGRATULATIONS! 🎉</p><p>You have been selected as the winner of our EXCLUSIVE giveaway!</p><p>Click below to claim your <strong>$10,000 prize</strong> now!</p><p><em>This is obviously spam.</em></p>`,
    date: d(-1, 6, 0),
    isRead: false,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'junk',
  },
  {
    id: '13',
    from: { name: 'Deals Today', email: 'offers@deals-today.spam', initials: 'DT', color: '#8a8a8a' },
    to: [{ name: 'You', email: 'you@company.com', initials: 'YO', color: '#0078d4' }],
    subject: 'URGENT: Limited time offer just for you!!!',
    preview: "Don't miss this incredible deal! 90% OFF on everything. This offer expires in 24 hours. Act now or regret forever…",
    body: `<p>🔥 URGENT: LIMITED TIME OFFER 🔥</p><p>90% OFF EVERYTHING!!!</p><p>This offer expires in 24 hours.</p><p><em>Definitely spam.</em></p>`,
    date: d(-2, 4, 30),
    isRead: false,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'junk',
  },
];

const workCalendarCategories: CalendarCategory[] = [
  { id: 'personal',  name: 'Calendar',        color: '#0078d4', visible: true, group: 'my' },
  { id: 'work',      name: 'Work',            color: '#498205', visible: true, group: 'my' },
  { id: 'birthdays', name: 'Birthdays',       color: '#e3008c', visible: true, group: 'other' },
  { id: 'holidays',  name: 'US Holidays',     color: '#da3b01', visible: true, group: 'other' },
];

const workCalendarEvents: CalendarEvent[] = [
  {
    id: 'evt-1',
    title: 'Weekly Standup',
    start: d(-2, 10, 0),
    end: d(-2, 10, 30),
    color: '#498205',
    location: 'Conference Room B',
    description: 'Weekly team standup to discuss progress and blockers.',
    isAllDay: false,
    calendarId: 'work',
    calendarName: 'Work',
    isOnlineMeeting: true,
    meetingUrl: 'https://teams.microsoft.com/l/meetup-join/mock',
  },
  {
    id: 'evt-2',
    title: 'Design Review',
    start: d(-2, 14, 0),
    end: d(-2, 15, 30),
    color: '#0078d4',
    location: 'Teams Meeting',
    description: 'Review the new design system components with Emily.',
    isAllDay: false,
    calendarId: 'personal',
    calendarName: 'Calendar',
    attendees: [
      { name: 'Emily Wang', email: 'emily.wang@company.com', initials: 'EW', color: '#8764b8', role: 'required' },
    ],
    isOnlineMeeting: true,
    meetingUrl: 'https://teams.microsoft.com/l/meetup-join/mock',
  },
  {
    id: 'evt-3',
    title: 'Lunch with Michael',
    start: d(-1, 12, 0),
    end: d(-1, 13, 0),
    color: '#498205',
    location: 'Café Uno',
    isAllDay: false,
    calendarId: 'work',
    calendarName: 'Work',
  },
  {
    id: 'evt-4',
    title: '1:1 with Roku Kaneshiro',
    start: d(-1, 15, 0),
    end: d(-1, 15, 30),
    color: '#498205',
    location: 'Office 301',
    description: 'Discuss Q2 budget and hiring plan.',
    isAllDay: false,
    calendarId: 'work',
    calendarName: 'Work',
    isOnlineMeeting: false,
  },
  {
    id: 'evt-5',
    title: 'Dentist Appointment',
    start: d(0, 9, 0),
    end: d(0, 10, 0),
    color: '#0078d4',
    location: 'Downtown Dental',
    isAllDay: false,
    calendarId: 'personal',
    calendarName: 'Calendar',
  },
  {
    id: 'evt-6',
    title: 'Sprint Planning',
    start: d(0, 13, 0),
    end: d(0, 14, 30),
    color: '#498205',
    location: 'Conference Room A',
    description: 'Plan next sprint — prioritize backlog items.',
    isAllDay: false,
    calendarId: 'work',
    calendarName: 'Work',
    isOnlineMeeting: true,
    meetingUrl: 'https://teams.microsoft.com/l/meetup-join/mock',
  },
  {
    id: 'evt-7',
    title: 'Product Roadmap Review',
    start: d(1, 9, 30),
    end: d(1, 11, 0),
    color: '#498205',
    location: 'Conference Room A / Teams',
    description: 'Review roadmap with Sara Chen.',
    isAllDay: false,
    calendarId: 'work',
    calendarName: 'Work',
    attendees: [
      { name: 'Sara Chen', email: 'sara.chen@company.com', initials: 'SC', color: '#0078d4', role: 'required' },
    ],
    isOnlineMeeting: true,
    meetingUrl: 'https://teams.microsoft.com/l/meetup-join/mock',
  },
  {
    id: 'evt-8',
    title: 'Gym',
    start: d(1, 17, 30),
    end: d(1, 18, 30),
    color: '#0078d4',
    isAllDay: false,
    calendarId: 'personal',
    calendarName: 'Calendar',
  },
  {
    id: 'evt-9',
    title: 'Team Lunch 🍕',
    start: d(2, 12, 0),
    end: d(2, 13, 0),
    color: '#498205',
    location: 'Local Pizza Place',
    description: 'Friday team lunch organized by Michael.',
    isAllDay: false,
    calendarId: 'work',
    calendarName: 'Work',
  },
  {
    id: 'evt-10',
    title: 'Project Phoenix Kickoff',
    start: d(5, 14, 0),
    end: d(5, 16, 0),
    color: '#498205',
    location: 'Conference Room A / Teams Link',
    description: 'Flagship product initiative kickoff meeting.',
    isAllDay: false,
    calendarId: 'work',
    calendarName: 'Work',
    attendees: [
      { name: 'Lisa Bergström', email: 'lisa.bergström@company.com', initials: 'LP', color: '#e3008c', role: 'required' },
      { name: 'Sara Chen', email: 'sara.chen@company.com', initials: 'SC', color: '#0078d4', role: 'required' },
      { name: 'Roku Kaneshiro', email: 'roku.kaneshiro@company.com', initials: 'DK', color: '#da3b01', role: 'required' },
    ],
    isOnlineMeeting: true,
    meetingUrl: 'https://teams.microsoft.com/l/meetup-join/mock',
  },
  {
    id: 'evt-11',
    title: "Lisa Bergström's Birthday",
    start: dAllDay(0),
    end: dAllDayEnd(0),
    color: '#e3008c',
    isAllDay: true,
    calendarId: 'birthdays',
    calendarName: 'Birthdays',
  },
  {
    id: 'evt-12',
    title: 'Product Demo — SyncSpace',
    start: d(7, 11, 0),
    end: d(7, 11, 45),
    color: '#0078d4',
    location: 'Online',
    description: 'Demo of SyncSpace collaboration platform by James Wilson.',
    isAllDay: false,
    calendarId: 'personal',
    calendarName: 'Calendar',
    isOnlineMeeting: true,
    meetingUrl: 'https://teams.microsoft.com/l/meetup-join/mock',
  },
];

const workContacts: FullContact[] = [
  {
    id: 'c-1',
    name: 'Alex Johnson',
    email: 'alex.j@company.com',
    initials: 'AJ',
    color: '#00b7c3',
    phone: '+1 (555) 234-5678',
    jobTitle: 'Engineering Lead',
    department: 'Engineering',
    organization: 'Cortexist Inc.',
    photoUrl: '/avatars/alex-johnson.jpg',
    isFavorite: true,
  },
  {
    id: 'c-2',
    name: 'Roku Kaneshiro',
    email: 'roku.kaneshiro@company.com',
    initials: 'DK',
    color: '#da3b01',
    phone: '+1 (555) 345-6789',
    mobile: '+1 (555) 345-0000',
    jobTitle: 'VP of Finance',
    department: 'Finance',
    organization: 'Cortexist Inc.',
    photoUrl: '/avatars/roku-kaneshiro.jpg',
    isFavorite: true,
  },
  {
    id: 'c-3',
    name: 'Emily Wang',
    email: 'emily.wang@company.com',
    initials: 'EW',
    color: '#8764b8',
    phone: '+1 (555) 456-7890',
    jobTitle: 'Senior Designer',
    department: 'Design',
    organization: 'Cortexist Inc.',
    isFavorite: true,
  },
  {
    id: 'c-4',
    name: 'James Wilson',
    email: 'james.w@partner.com',
    initials: 'JW',
    color: '#986f0b',
    phone: '+1 (555) 567-8901',
    jobTitle: 'Partner Relations',
    organization: 'SyncSpace',
    isFavorite: false,
  },
  {
    id: 'c-5',
    name: 'Lisa Bergström',
    email: 'lisa.bergström@company.com',
    initials: 'LP',
    color: '#e3008c',
    phone: '+1 (555) 678-9012',
    mobile: '+1 (555) 678-0000',
    jobTitle: 'Product Manager',
    department: 'Product',
    organization: 'Cortexist Inc.',
    birthday: 'March 4',
    photoUrl: '/avatars/lisa-bergstrom.jpg',
    isFavorite: true,
  },
  {
    id: 'c-6',
    name: 'Michael Torres',
    email: 'michael.t@company.com',
    initials: 'MT',
    color: '#498205',
    phone: '+1 (555) 789-0123',
    jobTitle: 'Software Engineer',
    department: 'Engineering',
    organization: 'Cortexist Inc.',
    isFavorite: false,
  },
  {
    id: 'c-7',
    name: 'Rachel Adams',
    email: 'rachel.a@company.com',
    initials: 'RA',
    color: '#c239b3',
    phone: '+1 (555) 890-1234',
    jobTitle: 'HR Business Partner',
    department: 'Human Resources',
    organization: 'Cortexist Inc.',
    isFavorite: false,
  },
  {
    id: 'c-8',
    name: 'Sara Chen',
    email: 'sara.chen@company.com',
    initials: 'SC',
    color: '#0078d4',
    phone: '+1 (555) 123-4567',
    mobile: '+1 (555) 123-0000',
    jobTitle: 'Product Director',
    department: 'Product',
    organization: 'Cortexist Inc.',
    photoUrl: '/avatars/sara-chen.jpg',
    isFavorite: true,
  },
  {
    id: 'c-9',
    name: 'Tom Martinez',
    email: 'tom.m@company.com',
    initials: 'TM',
    color: '#107c10',
    phone: '+1 (555) 901-2345',
    jobTitle: 'DevOps Engineer',
    department: 'Engineering',
    organization: 'Cortexist Inc.',
    isFavorite: false,
  },
  {
    id: 'c-10',
    name: 'Victoria Lee',
    email: 'victoria.l@company.com',
    initials: 'VL',
    color: '#7719aa',
    phone: '+1 (555) 012-3456',
    jobTitle: 'QA Lead',
    department: 'Engineering',
    organization: 'Cortexist Inc.',
    isFavorite: false,
  },
];

/* ═══════════════════════════════════════════════════════
   ACCOUNT 2 — Personal (Gmail)
   ═══════════════════════════════════════════════════════ */

const personalFolders: Folder[] = [
  { id: 'inbox',   name: 'Inbox',         icon: 'inbox',   isFavorite: true  },
  { id: 'sent',    name: 'Sent Items',    icon: 'sent',    isFavorite: true  },
  { id: 'drafts',  name: 'Drafts',        icon: 'drafts',  isFavorite: true  },
  { id: 'deleted', name: 'Deleted Items', icon: 'deleted', isFavorite: false },
  { id: 'junk',    name: 'Junk Email',    icon: 'junk',    isFavorite: false },
  { id: 'archive', name: 'Archive',       icon: 'archive', isFavorite: false },
];

const personalEmails: Email[] = [
  {
    id: 'p1',
    from: { name: 'Mom', email: 'mom@gmail.com', initials: 'MR', color: '#e3008c' },
    to: [{ name: 'You', email: 'santoshi.nakamoto@gmail.com', initials: 'AR', color: '#498205' }],
    subject: 'Dinner this Sunday? 🍝',
    preview: "Hi sweetheart! Your dad and I were thinking of having a family dinner this Sunday. Can you make it?",
    body: `<p>Hi sweetheart!</p>
<p>Your dad and I were thinking of having a family dinner this Sunday around 6 PM. I'm making your favorite lasagna! 🍝</p>
<p>Can you make it? Let me know if you're bringing anyone.</p>
<p>Love,<br/>Mom</p>`,
    date: d(0, 12, 0),
    isRead: false,
    isStarred: true,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  {
    id: 'p2',
    from: { name: 'Netflix', email: 'info@netflix.com', initials: 'NF', color: '#e50914' },
    to: [{ name: 'You', email: 'santoshi.nakamoto@gmail.com', initials: 'AR', color: '#498205' }],
    subject: 'New arrivals this week on Netflix',
    preview: "Check out what's new this week — including the highly anticipated Season 3 of your favorite show!",
    body: `<p>Hi Alex,</p>
<p>Here's what's new this week on Netflix:</p>
<ul>
  <li><strong>Cyber Drift: Season 3</strong> — The crew returns for their biggest heist yet</li>
  <li><strong>Mountain Echo</strong> — Award-winning documentary</li>
  <li><strong>Last Laugh</strong> — New stand-up comedy special</li>
</ul>
<p>Start watching today!</p>`,
    date: d(0, 6, 0),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  {
    id: 'p3',
    from: { name: 'Jake Rivera', email: 'jake.r@gmail.com', initials: 'JR', color: '#00b7c3' },
    to: [{ name: 'You', email: 'santoshi.nakamoto@gmail.com', initials: 'AR', color: '#498205' }],
    subject: 'Road trip next month?',
    preview: "Hey! I was thinking we could do a road trip to the mountains next month. What do you think?",
    body: `<p>Hey!</p>
<p>I was thinking we could do a road trip to the mountains next month. Maybe a long weekend?</p>
<p><strong>Ideas:</strong></p>
<ul>
  <li>Blue Ridge Parkway</li>
  <li>Shenandoah National Park</li>
  <li>Great Smoky Mountains</li>
</ul>
<p>Let me know if you're in! 🏔️</p>
<p>— Jake</p>`,
    date: d(-1, 20, 30),
    isRead: false,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  {
    id: 'p4',
    from: { name: 'Amazon', email: 'ship-confirm@amazon.com', initials: 'AZ', color: '#ff9900' },
    to: [{ name: 'You', email: 'santoshi.nakamoto@gmail.com', initials: 'AR', color: '#498205' }],
    subject: 'Your order has shipped! 📦',
    preview: 'Great news — your order #112-7429816 has shipped and is on its way. Expected delivery: in 2 days.',
    body: `<p>Hello Alex,</p>
<p>Great news! Your order has shipped.</p>
<p><strong>Order #112-7429816</strong></p>
<ul>
  <li>Sony WH-1000XM5 Headphones</li>
  <li>Expected delivery: <strong>in 2 days</strong></li>
</ul>
<p>Track your package in the Amazon app.</p>`,
    date: d(-1, 14, 0),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  {
    id: 'p5',
    from: { name: 'Mia Chen', email: 'mia.c@gmail.com', initials: 'MC', color: '#8764b8' },
    to: [{ name: 'You', email: 'santoshi.nakamoto@gmail.com', initials: 'AR', color: '#498205' }],
    subject: 'Photos from last weekend 📸',
    preview: "Here are the photos from the hiking trip! Some really great shots. Check out the album link…",
    body: `<p>Hey Alex!</p>
<p>Here are the photos from Saturday's hike. Some really great shots came out!</p>
<p>I uploaded them to Google Photos — here's the album link.</p>
<p>My favorites are the sunset ones from the summit. 🌅</p>
<p>— Mia</p>`,
    date: d(-2, 18, 0),
    isRead: true,
    isStarred: true,
    isPinned: false,
    hasAttachment: true,
    folder: 'inbox',
  },
  {
    id: 'p6',
    from: { name: 'You', email: 'santoshi.nakamoto@gmail.com', initials: 'AR', color: '#498205' },
    to: [{ name: 'Jake Rivera', email: 'jake.r@gmail.com', initials: 'JR', color: '#00b7c3' }],
    subject: 'Re: Road trip next month?',
    preview: "I'm totally in! Shenandoah gets my vote. Let's start planning!",
    body: `<p>I'm totally in! Shenandoah gets my vote — the trails there are amazing in spring.</p>
<p>Let's start planning this weekend!</p>`,
    date: d(-1, 21, 0),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'sent',
  },
];

const personalCalendarCategories: CalendarCategory[] = [
  { id: 'personal',  name: 'Personal',    color: '#498205', visible: true, group: 'my' },
  { id: 'family',    name: 'Family',      color: '#e3008c', visible: true, group: 'my' },
  { id: 'social',    name: 'Social',      color: '#00b7c3', visible: true, group: 'my' },
];

const personalCalendarEvents: CalendarEvent[] = [
  {
    id: 'pevt-1',
    title: 'Family Dinner',
    start: d(3, 18, 0),
    end: d(3, 20, 0),
    color: '#e3008c',
    location: "Mom & Dad's house",
    description: 'Sunday family dinner — lasagna!',
    isAllDay: false,
    calendarId: 'family',
    calendarName: 'Family',
  },
  {
    id: 'pevt-2',
    title: 'Gym — Leg Day',
    start: d(-1, 7, 0),
    end: d(-1, 8, 0),
    color: '#498205',
    location: 'FitZone Gym',
    isAllDay: false,
    calendarId: 'personal',
    calendarName: 'Personal',
  },
  {
    id: 'pevt-3',
    title: 'Movie Night with Mia',
    start: d(2, 19, 0),
    end: d(2, 22, 0),
    color: '#00b7c3',
    location: 'AMC Theater',
    isAllDay: false,
    calendarId: 'social',
    calendarName: 'Social',
  },
  {
    id: 'pevt-4',
    title: 'Car Service Appointment',
    start: d(1, 8, 0),
    end: d(1, 9, 30),
    color: '#498205',
    location: 'AutoCare Express',
    isAllDay: false,
    calendarId: 'personal',
    calendarName: 'Personal',
  },
  {
    id: 'pevt-5',
    title: "Jake's Birthday",
    start: dAllDay(7),
    end: dAllDayEnd(7),
    color: '#e3008c',
    isAllDay: true,
    calendarId: 'family',
    calendarName: 'Family',
  },
];

const personalContacts: FullContact[] = [
  {
    id: 'pc-1',
    name: 'Mom (Maria Nakamoto)',
    email: 'mom@gmail.com',
    initials: 'MR',
    color: '#e3008c',
    phone: '+1 (555) 100-1000',
    mobile: '+1 (555) 100-1001',
    notes: '❤️',
    photoUrl: '/avatars/mom.jpg',
    isFavorite: true,
  },
  {
    id: 'pc-2',
    name: 'Jung-Jae Lee',
    email: 'jung-jae.lee@gmail.com',
    initials: 'CR',
    color: '#da3b01',
    phone: '+1 (555) 100-2000',
    isFavorite: true,
  },
  {
    id: 'pc-3',
    name: 'Jake Rivera',
    email: 'jake.r@gmail.com',
    initials: 'JR',
    color: '#00b7c3',
    phone: '+1 (555) 200-3000',
    mobile: '+1 (555) 200-3001',
    birthday: 'March 12',
    notes: 'Brother',
    isFavorite: true,
  },
  {
    id: 'pc-4',
    name: 'Mia Chen',
    email: 'mia.c@gmail.com',
    initials: 'MC',
    color: '#8764b8',
    phone: '+1 (555) 300-4000',
    isFavorite: true,
  },
  {
    id: 'pc-5',
    name: 'Dr. Patel',
    email: 'office@downtowndental.com',
    initials: 'DP',
    color: '#986f0b',
    phone: '+1 (555) 400-5000',
    jobTitle: 'Dentist',
    organization: 'Downtown Dental',
    isFavorite: false,
  },
];

/* ═══════════════════════════════════════════════════════
   ACCOUNT 3 — Side Project (Indie Dev)
   ═══════════════════════════════════════════════════════ */

const sideFolders: Folder[] = [
  { id: 'inbox',   name: 'Inbox',         icon: 'inbox',   isFavorite: true  },
  { id: 'sent',    name: 'Sent Items',    icon: 'sent',    isFavorite: true  },
  { id: 'drafts',  name: 'Drafts',        icon: 'drafts',  isFavorite: true  },
  { id: 'deleted', name: 'Deleted Items', icon: 'deleted', isFavorite: false },
  { id: 'junk',    name: 'Junk Email',    icon: 'junk',    isFavorite: false },
  { id: 'archive', name: 'Archive',       icon: 'archive', isFavorite: false },
];

const sideEmails: Email[] = [
  {
    id: 's1',
    from: { name: 'GitHub', email: 'noreply@github.com', initials: 'GH', color: '#24292e' },
    to: [{ name: 'You', email: 'alex@pixelforge.dev', initials: 'PF', color: '#7719aa' }],
    subject: '[pixelforge/aurora] Issue #47: Dark mode flickers on startup',
    preview: 'New issue opened by @codemaster99: "When launching the app in dark mode, there is a brief white flash before the theme applies…"',
    body: `<p><strong>@codemaster99</strong> opened issue <strong>#47</strong> in <strong>pixelforge/aurora</strong>:</p>
<p>When launching the app in dark mode, there's a brief white flash before the theme applies. This happens on both Windows and macOS.</p>
<p><strong>Steps to reproduce:</strong></p>
<ol>
  <li>Enable system dark mode</li>
  <li>Launch Aurora</li>
  <li>Observe brief white flash</li>
</ol>
<p><strong>Expected:</strong> App should launch directly in dark mode without flash.</p>`,
    date: d(0, 14, 0),
    isRead: false,
    isStarred: true,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  {
    id: 's2',
    from: { name: 'Stripe', email: 'notifications@stripe.com', initials: 'ST', color: '#635bff' },
    to: [{ name: 'You', email: 'alex@pixelforge.dev', initials: 'PF', color: '#7719aa' }],
    subject: 'Your monthly payout has been sent',
    preview: 'A payout of $1,284.50 has been initiated to your bank account ending in 4821. Expected arrival: in 2 days.',
    body: `<p>Hello PixelForge,</p>
<p>Your monthly payout has been processed:</p>
<ul>
  <li><strong>Amount:</strong> $1,284.50</li>
  <li><strong>Bank account:</strong> ****4821</li>
  <li><strong>Expected arrival:</strong> in 2 days</li>
</ul>
<p>View your full payout details on the Stripe Dashboard.</p>`,
    date: d(-1, 9, 0),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  {
    id: 's3',
    from: { name: 'Nina Patel', email: 'nina@designcraft.io', initials: 'NP', color: '#e3008c' },
    to: [{ name: 'You', email: 'alex@pixelforge.dev', initials: 'PF', color: '#7719aa' }],
    subject: 'Redesigned icons for Aurora v2 🎨',
    preview: "Hey Alex! I finished the new icon set for Aurora v2. Attached is the full SVG pack — let me know what you think!",
    body: `<p>Hey Alex!</p>
<p>I finished the new icon set for Aurora v2. The pack includes:</p>
<ul>
  <li>48 app icons (light + dark variants)</li>
  <li>24 toolbar icons</li>
  <li>12 status icons</li>
</ul>
<p>Attached as SVG. I also included a Figma link in the project channel.</p>
<p>Let me know what you think! 🎨</p>
<p>— Nina</p>`,
    date: d(-2, 15, 30),
    isRead: false,
    isStarred: true,
    isPinned: false,
    hasAttachment: true,
    folder: 'inbox',
  },
  {
    id: 's4',
    from: { name: 'Product Hunt', email: 'noreply@producthunt.com', initials: 'PH', color: '#da552f' },
    to: [{ name: 'You', email: 'alex@pixelforge.dev', initials: 'PF', color: '#7719aa' }],
    subject: '🎉 Aurora is trending on Product Hunt!',
    preview: "Congrats! Aurora has been featured and is currently #3 on Product Hunt with 342 upvotes!",
    body: `<p>🎉 Congratulations!</p>
<p><strong>Aurora</strong> by PixelForge is trending on Product Hunt!</p>
<ul>
  <li><strong>Current rank:</strong> #3</li>
  <li><strong>Upvotes:</strong> 342</li>
  <li><strong>Comments:</strong> 28</li>
</ul>
<p>Keep engaging with the community — your launch is going great!</p>`,
    date: d(-3, 10, 0),
    isRead: true,
    isStarred: true,
    isPinned: false,
    hasAttachment: false,
    folder: 'inbox',
  },
  {
    id: 's5',
    from: { name: 'You', email: 'alex@pixelforge.dev', initials: 'PF', color: '#7719aa' },
    to: [{ name: 'Nina Patel', email: 'nina@designcraft.io', initials: 'NP', color: '#e3008c' }],
    subject: 'Re: Redesigned icons for Aurora v2 🎨',
    preview: "These look incredible! I love the new toolbar set. A few tweaks on the status icons…",
    body: `<p>Nina, these look incredible!</p>
<p>I especially love the new toolbar set — the weight feels just right. A few notes on the status icons:</p>
<ul>
  <li>The "offline" icon could use a bit more contrast</li>
  <li>Love the "syncing" animation concept</li>
</ul>
<p>I'll push these into the dev branch this weekend. Thanks!</p>`,
    date: d(-2, 17, 0),
    isRead: true,
    isStarred: false,
    isPinned: false,
    hasAttachment: false,
    folder: 'sent',
  },
];

const sideCalendarCategories: CalendarCategory[] = [
  { id: 'dev',       name: 'Development',  color: '#7719aa', visible: true, group: 'my' },
  { id: 'marketing', name: 'Marketing',    color: '#da552f', visible: true, group: 'my' },
  { id: 'meetings',  name: 'Meetings',     color: '#0078d4', visible: true, group: 'my' },
];

const sideCalendarEvents: CalendarEvent[] = [
  {
    id: 'sevt-1',
    title: 'Aurora v2 Sprint',
    start: d(-2, 9, 0),
    end: d(-2, 12, 0),
    color: '#7719aa',
    description: 'Focus block: dark mode fix + new icon integration.',
    isAllDay: false,
    calendarId: 'dev',
    calendarName: 'Development',
  },
  {
    id: 'sevt-2',
    title: 'Call with Nina — Icon Review',
    start: d(0, 14, 0),
    end: d(0, 14, 30),
    color: '#0078d4',
    location: 'Google Meet',
    isAllDay: false,
    calendarId: 'meetings',
    calendarName: 'Meetings',
    isOnlineMeeting: true,
    meetingUrl: 'https://teams.microsoft.com/l/meetup-join/mock',
  },
  {
    id: 'sevt-3',
    title: 'Product Hunt Launch Day',
    start: dAllDay(-3),
    end: dAllDayEnd(-3),
    color: '#da552f',
    description: 'Aurora launches on Product Hunt!',
    isAllDay: true,
    calendarId: 'marketing',
    calendarName: 'Marketing',
  },
  {
    id: 'sevt-4',
    title: 'Write blog post: Aurora v2 roadmap',
    start: d(2, 10, 0),
    end: d(2, 12, 0),
    color: '#da552f',
    isAllDay: false,
    calendarId: 'marketing',
    calendarName: 'Marketing',
  },
  {
    id: 'sevt-5',
    title: 'Community AMA on Discord',
    start: d(3, 16, 0),
    end: d(3, 17, 0),
    color: '#0078d4',
    location: 'Discord #general',
    description: 'Answer community questions about Aurora v2.',
    isAllDay: false,
    calendarId: 'meetings',
    calendarName: 'Meetings',
  },
];

const sideContacts: FullContact[] = [
  {
    id: 'sc-1',
    name: 'Nina Patel',
    email: 'nina@designcraft.io',
    initials: 'NP',
    color: '#e3008c',
    phone: '+1 (555) 500-6000',
    jobTitle: 'Freelance Designer',
    organization: 'DesignCraft',
    photoUrl: '/avatars/nina-patel.jpg',
    isFavorite: true,
  },
  {
    id: 'sc-2',
    name: 'Ryan Kim',
    email: 'ryan@betatesters.co',
    initials: 'RK',
    color: '#00b7c3',
    jobTitle: 'Beta Testing Lead',
    organization: 'BetaTesters Co.',
    isFavorite: false,
  },
  {
    id: 'sc-3',
    name: 'Stripe Support',
    email: 'support@stripe.com',
    initials: 'SS',
    color: '#635bff',
    organization: 'Stripe',
    isFavorite: false,
  },
];

/* ═══════════════════════════════════════════════════════
   ACCOUNTS — Exported
   ═══════════════════════════════════════════════════════ */

export const mockAccounts: Account[] = [
  {
    id: 'work',
    name: 'Santoshi Nakamoto',
    email: 'santoshi.nakamoto@cortexist.com',
    initials: 'AR',
    color: '#0078d4',
    avatarUrl: '/avatars/santoshi-nakamoto.jpg',
    folders: workFolders,
    emails: withMockFocusedFlag(workEmails),
    calendarEvents: workCalendarEvents,
    calendarCategories: workCalendarCategories,
    contacts: workContacts,
  },
  {
    id: 'personal',
    name: 'Santoshi Nakamoto',
    email: 'santoshi.nakamoto@gmail.com',
    initials: 'AR',
    color: '#498205',
    folders: personalFolders,
    emails: withMockFocusedFlag(personalEmails),
    calendarEvents: personalCalendarEvents,
    calendarCategories: personalCalendarCategories,
    contacts: personalContacts,
  },
  {
    id: 'side',
    name: 'PixelForge',
    email: 'alex@pixelforge.dev',
    initials: 'PF',
    color: '#7719aa',
    folders: sideFolders,
    emails: withMockFocusedFlag(sideEmails),
    calendarEvents: sideCalendarEvents,
    calendarCategories: sideCalendarCategories,
    contacts: sideContacts,
  },
];

/* Legacy exports — point to first (work) account */
export const mockFolders = workFolders;
export const mockEmails = withMockFocusedFlag(workEmails);
export const mockCalendarCategories = workCalendarCategories;
export const mockCalendarEvents = workCalendarEvents;
export const mockContacts = workContacts;
