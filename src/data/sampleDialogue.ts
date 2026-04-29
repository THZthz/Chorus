import { DialogueStep } from '@/types/dialogue';

export const sampleDialogue: Record<string, DialogueStep> = {
  start: {
    id: 'start',
    messages: [
      {
        speaker: 'HALF LIGHT',
        type: 'INNER_VOICE',
        text: 'The smell of rot and cheap incense hit you like a physical blow. Somewhere in the shadows, a blade is being sharpened. You skip a heartbeat. *Do not turn your back.*',
        skillCheck: {
          skill: 'Half Light',
          difficulty: 'Easy',
          success: true
        }
      },
      {
        speaker: 'SYSTEM',
        type: 'SYSTEM',
        text: 'You push past the heavy leather curtain into the main hall of [The Crimson Veil](#crimson_veil). The air is a thick soup of tallow smoke and unwashed bodies.'
      },
      {
        speaker: 'MADAM VESPERA',
        type: 'CHARACTER',
        text: '"We don\'t see your kind often, stranger. Too much polish on those boots for the mud of the lower district. What are you looking for? Solace? Or a place to hide?"'
      },
      {
        speaker: 'INLAND EMPIRE',
        type: 'INNER_VOICE',
        text: 'Her [velvet choker](#velvet_choker) pulsates. A tiny, rhythmic throb. It remembers the throats of the dead. It wants to tell you about the plague.',
        skillCheck: {
          skill: 'Inland Empire',
          difficulty: 'Medium',
          success: true
        }
      },
      {
        speaker: 'YOU',
        type: 'YOU',
        text: '"I am not here for solace, Madam. I am here for a man. They call him the Red Knight."'
      },
      {
        speaker: 'MADAM VESPERA',
        type: 'CHARACTER',
        text: '"The Red Knight?" She spits onto the rush-covered floor. "A name like that costs more than a [rusted coin](#rusted_coin). Information is the only currency I value more than silver, and you don\'t look like you have much of either."'
      }
    ],
    options: [
      {
        id: 'opt_charm',
        text: '"Surely a woman of your... elegance... can appreciate a shared interest."',
        hintAfter: '[Charm her.]',
        check: {
          skill: 'Suggestion',
          difficulty: 10,
          difficultyText: 'Challenging',
          diceCount: 2,
          conditions: [
            {
              expression: "dice[0] === 1 && dice[1] === 1",
              stepId: 'charm_fail_crit',
              label: 'Critical Failure',
              color: 'text-red-700'
            },
            {
              expression: "success",
              stepId: 'charm_success',
              label: 'Success',
              color: 'text-green-500'
            },
            {
              expression: "true",
              stepId: 'charm_failure',
              label: 'Failure',
              color: 'text-red-500'
            }
          ]
        }
      },
      {
        id: 'opt_threat',
        text: '"Tell me what I want to know, or I\'ll burn this rat-nest to the ground."',
        hintAfter: '[Intimidate.]',
        check: {
          skill: 'Half Light',
          difficulty: 12,
          difficultyText: 'Hard',
          diceCount: 2,
          conditions: [
            {
              expression: "success",
              stepId: 'intimidate_success',
              label: 'Success',
              color: 'text-green-500'
            },
            {
              expression: "true",
              stepId: 'intimidate_failure',
              label: 'Failure',
              color: 'text-red-500'
            }
          ]
        }
      },
      {
        id: 'opt_ai',
        text: '"Who is the Red Knight, really?"',
        hintBefore: '[Consult the Void]',
        isAiTrigger: true
      },
      {
        id: 'opt_leave',
        text: '"I\'ve seen enough. Farewell."',
        nextStepId: 'leave'
      },
      {
        id: 'opt_demo',
        text: '',
        hintBefore: '[Demo: Show me high-stakes UI and notifications.]',
        nextStepId: 'red_check_demo'
      }
    ]
  },
  red_check_demo: {
    id: 'red_check_demo',
    messages: [
      {
        speaker: 'LOGIC',
        type: 'INNER_VOICE',
        text: 'This is a demonstration of the new visual systems. Observe the notifications following this message.'
      },
      {
        speaker: 'SYSTEM',
        type: 'NOTIFICATION',
        text: '+5 XP: gained experience.'
      },
      {
        speaker: 'SYSTEM',
        type: 'NOTIFICATION',
        text: 'New task: Interview cafeteria manager'
      },
      {
        speaker: 'SYSTEM',
        type: 'NOTIFICATION',
        text: 'Item gained: Horrific Necktie'
      }
    ],
    options: [
      {
        id: 'opt_red_check',
        text: 'Try to invent a name for yourself.',
        check: {
          skill: 'Conceptualization',
          difficulty: 11,
          difficultyText: 'Medium',
          diceCount: 2,
          isRed: true,
          conditions: [
            { expression: "success", stepId: 'red_success', label: 'Success' },
            { expression: "true", stepId: 'red_fail', label: 'Failure' }
          ]
        }
      }
    ]
  },
  red_success: {
    id: 'red_success',
    messages: [
      {
        speaker: 'CONCEPTUALIZATION',
        type: 'INNER_VOICE',
        text: 'You are... *Raphaël Ambrosius Costeau*. Yes. That sounds like a name for an officer of the law. Or a very confused disco dancer.'
      }
    ],
    options: [
      { id: 'cont', text: 'CONTINUE', isContinue: true, nextStepId: 'start' }
    ]
  },
  red_fail: {
    id: 'red_fail',
    messages: [
      {
        speaker: 'CONCEPTUALIZATION',
        type: 'INNER_VOICE',
        text: 'Nothing. Your mind is a blank slate of gray static. You are just... a person. A person without a name.'
      }
    ],
    options: [
      { id: 'cont', text: 'CONTINUE', isContinue: true, nextStepId: 'start' }
    ]
  },
  charm_success: {
    id: 'charm_success',
    messages: [
      {
        speaker: 'SUGGESTION',
        type: 'INNER_VOICE',
        text: 'The ice in her gaze thaws, just a fraction. You caught the ghost of a smile.',
        skillCheck: { skill: 'Suggestion', difficulty: 'Challenging', success: true }
      },
      {
        speaker: 'MADAM VESPERA',
        type: 'CHARACTER',
        text: '"You have a silver tongue, stranger. Dangerous. The Red Knight was here, yes. He left an hour before the bells of Saint Agnes rang. Headed for the Iron Docks. Said he had a debt of blood to settle."'
      }
    ],
    options: [
      { id: 'back', text: '"Thank you, Madam. One more thing..."', nextStepId: 'start' }
    ]
  },
  charm_failure: {
    id: 'charm_failure',
    messages: [
      {
        speaker: 'SUGGESTION',
        type: 'INNER_VOICE',
        text: 'Your words land like wet clay. She looks at you as if you\'re a particularly dull child.',
        skillCheck: { skill: 'Suggestion', difficulty: 'Challenging', success: false }
      },
      {
        speaker: 'MADAM VESPERA',
        type: 'CHARACTER',
        text: '"Save your flattery for the girls upstairs, boy. They\'re paid to pretend it works. I\'m not."'
      }
    ],
    options: [
      { id: 'back', text: '"Apologies. Let me try another way."', nextStepId: 'start' }
    ]
  },
  charm_fail_crit: {
    id: 'charm_fail_crit',
    messages: [
      {
        speaker: 'SUGGESTION',
        type: 'INNER_VOICE',
        text: 'CRITICAL FAILURE. You stutter, your voice hitting a high-pitched note that makes a nearby drunk choke on his ale. Vespera\'s hand drops to the heavy ring of keys at her belt.',
        skillCheck: { skill: 'Suggestion', difficulty: 'Challenging', success: false }
      },
      {
        speaker: 'MADAM VESPERA',
        type: 'CHARACTER',
        text: '"Get out. Before I have the boys show you how we handle jesters in this house. You\'re not half as clever as you think you are."'
      }
    ],
    options: [
      { id: 'leave', text: '', hintBefore: '[Leave quickly before things get violent.]', nextStepId: 'leave' }
    ]
  },
  intimidate_success: {
    id: 'intimidate_success',
    messages: [
      {
        speaker: 'HALF LIGHT',
        type: 'INNER_VOICE',
        text: 'The darkness in your voice matches the darkness in the room. She sees the killer in you.',
        skillCheck: { skill: 'Half Light', difficulty: 'Hard', success: true }
      },
      {
        speaker: 'MADAM VESPERA',
        type: 'CHARACTER',
        text: '"Easy, wolf. No need to howl. The Red Knight is a regular. He\'s currently in the cellar, "negotiating" with some of my boys. If you want him, go. I won\'t weep if he doesn\'t come back up."'
      }
    ],
    options: [
      { id: 'back', text: '"Wise choice."', nextStepId: 'start' }
    ]
  },
  intimidate_failure: {
    id: 'intimidate_failure',
    messages: [
      {
        speaker: 'HALF LIGHT',
        type: 'INNER_VOICE',
        text: 'Your voice cracks. The threat feels hollow in the face of her decades of dealing with much worse men.',
        skillCheck: { skill: 'Half Light', difficulty: 'Hard', success: false }
      },
      {
        speaker: 'MADAM VESPERA',
        type: 'CHARACTER',
        text: '"You think you\'re the first person to threaten this house? I’ve watched kings rot in my gutters. Don\'t test my patience before the guards drag you out to the pillory."'
      }
    ],
    options: [
      { id: 'back', text: '"Fine. We\'ll see."', nextStepId: 'start' }
    ]
  },
  leave: {
    id: 'leave',
    messages: [
      {
        speaker: 'SYSTEM',
        type: 'SYSTEM',
        text: 'You turn and walk back into the biting cold of the medieval night. The heavy curtain falls shut, muffled laughter and the clink of stone mugs fading behind you.'
      }
    ],
    options: [
      {
        id: 'restart',
        text: '',
        hintBefore: '[Re-enter the Veil]',
        nextStepId: 'start'
      }
    ]
  }
};
