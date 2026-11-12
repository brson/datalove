// Recorded by crates/datalove-doc-fixtures from replays/reactive-revive.world.
// Do not edit: run `BLESS=1 cargo test -p datalove-doc-fixtures`.
DataloveReplay.register("reactive-revive", {
  "version": 1,
  "steps": [
    {
      "input": "var b = 125",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Slot": {
                "name": "b",
                "ty": "int",
                "value": "125"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 125",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [
              [
                "b",
                "125"
              ]
            ],
            "provides": [
              "b"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": "125"
        }
      ]
    },
    {
      "input": "let b_moved = b",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "b_moved",
                "ty": "int",
                "value": "125"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 125",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [
              [
                "b",
                null
              ]
            ],
            "provides": [
              "b"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 2,
          "expression": false,
          "source": "let b_moved = b",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b_moved",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [
              [
                "b_moved",
                "125"
              ]
            ],
            "provides": [
              "b_moved"
            ],
            "reads": [],
            "takes": [
              "b"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": null
        },
        {
          "name": "b_moved",
          "ty": "int",
          "value": "125"
        }
      ]
    },
    {
      "input": "set b = 500",
      "outcome": {
        "Unit": {
          "Ran": []
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 125",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [
              [
                "b",
                "500"
              ]
            ],
            "provides": [
              "b"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 2,
          "expression": false,
          "source": "let b_moved = b",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b_moved",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [
              [
                "b_moved",
                "125"
              ]
            ],
            "provides": [
              "b_moved"
            ],
            "reads": [],
            "takes": [
              "b"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "set b = 500",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": [
              "b"
            ]
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": "500"
        },
        {
          "name": "b_moved",
          "ty": "int",
          "value": "125"
        }
      ]
    },
    {
      "input": "let c = b + 1",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "c",
                "ty": "int",
                "value": "501"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 125",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [
              [
                "b",
                "500"
              ]
            ],
            "provides": [
              "b"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 2,
          "expression": false,
          "source": "let b_moved = b",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b_moved",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [
              [
                "b_moved",
                "125"
              ]
            ],
            "provides": [
              "b_moved"
            ],
            "reads": [],
            "takes": [
              "b"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "set b = 500",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": [
              "b"
            ]
          }
        },
        {
          "number": 4,
          "expression": false,
          "source": "let c = b + 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "501"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 4,
            "diagnostics": "",
            "bindings": [
              [
                "c",
                "501"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "b"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": "500"
        },
        {
          "name": "b_moved",
          "ty": "int",
          "value": "125"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "501"
        }
      ]
    },
    {
      "input": "/edit 3 set b = 500\nlet x = y",
      "outcome": {
        "Edited": [
          {
            "unit": 1,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Slot": {
                    "name": "b",
                    "ty": "int",
                    "value": "125"
                  }
                }
              ]
            }
          },
          {
            "unit": 2,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "b_moved",
                    "ty": "int",
                    "value": "125"
                  }
                }
              ]
            }
          },
          {
            "unit": 3,
            "compiled": true,
            "eval": {
              "NotCompiled": "cannot find `y` in this scope"
            }
          },
          {
            "unit": 4,
            "compiled": true,
            "eval": {
              "NotCompiled": "error[O013]: `b` was given away by an earlier unit\n  help: use `@` to clone `b` where it was given away"
            }
          }
        ]
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 125",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [
              [
                "b",
                null
              ]
            ],
            "provides": [
              "b"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 2,
          "expression": false,
          "source": "let b_moved = b",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b_moved",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [
              [
                "b_moved",
                "125"
              ]
            ],
            "provides": [
              "b_moved"
            ],
            "reads": [],
            "takes": [
              "b"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "set b = 500\nlet x = y",
          "status": "NotCompiled",
          "outcome": {
            "NotCompiled": "cannot find `y` in this scope"
          },
          "compiled": true,
          "ran": false,
          "detail": {
            "unit": 3,
            "diagnostics": "\u001b[31m[F001] Error:\u001b[0m cannot find value `y` in this scope\n   \u001b[38;5;246m╭\u001b[0m\u001b[38;5;246m─\u001b[0m\u001b[38;5;246m[\u001b[0m unit 3:2:9 \u001b[38;5;246m]\u001b[0m\n   \u001b[38;5;246m│\u001b[0m\n \u001b[38;5;246m2 │\u001b[0m \u001b[38;5;249ml\u001b[0m\u001b[38;5;249me\u001b[0m\u001b[38;5;249mt\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249mx\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m=\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;201my\u001b[0m\n \u001b[38;5;240m  │\u001b[0m         \u001b[38;5;201m┬\u001b[0m  \n \u001b[38;5;240m  │\u001b[0m         \u001b[38;5;201m╰\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m not found in this scope\n\u001b[38;5;246m───╯\u001b[0m\n",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 4,
          "expression": false,
          "source": "let c = b + 1",
          "status": "NotCompiled",
          "outcome": {
            "NotCompiled": "error[O013]: `b` was given away by an earlier unit\n  help: use `@` to clone `b` where it was given away"
          },
          "compiled": true,
          "ran": false,
          "detail": {
            "unit": 4,
            "diagnostics": "\u001b[31m[O013] Error:\u001b[0m `b` was given away by an earlier unit\n   \u001b[38;5;246m╭\u001b[0m\u001b[38;5;246m─\u001b[0m\u001b[38;5;246m[\u001b[0m unit 4:1:9 \u001b[38;5;246m]\u001b[0m\n   \u001b[38;5;246m│\u001b[0m\n \u001b[38;5;246m1 │\u001b[0m \u001b[38;5;249ml\u001b[0m\u001b[38;5;249me\u001b[0m\u001b[38;5;249mt\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249mc\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m=\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;201mb\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m+\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m1\u001b[0m\n \u001b[38;5;240m  │\u001b[0m         \u001b[38;5;201m┬\u001b[0m  \n \u001b[38;5;240m  │\u001b[0m         \u001b[38;5;201m╰\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m used after it was given away\n \u001b[38;5;240m  │\u001b[0m \n \u001b[38;5;240m  │\u001b[0m \u001b[38;5;115mHelp\u001b[0m: use `@` to clone `b` where it was given away\n\u001b[38;5;246m───╯\u001b[0m\n",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": null
        },
        {
          "name": "b_moved",
          "ty": "int",
          "value": "125"
        }
      ]
    },
    {
      "input": "/edit 3 set b = 500",
      "outcome": {
        "Edited": [
          {
            "unit": 3,
            "compiled": true,
            "eval": {
              "Ran": []
            }
          },
          {
            "unit": 4,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "c",
                    "ty": "int",
                    "value": "501"
                  }
                }
              ]
            }
          }
        ]
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 125",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [
              [
                "b",
                "500"
              ]
            ],
            "provides": [
              "b"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 2,
          "expression": false,
          "source": "let b_moved = b",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b_moved",
                  "ty": "int",
                  "value": "125"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [
              [
                "b_moved",
                "125"
              ]
            ],
            "provides": [
              "b_moved"
            ],
            "reads": [],
            "takes": [
              "b"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "set b = 500",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": [
              "b"
            ]
          }
        },
        {
          "number": 4,
          "expression": false,
          "source": "let c = b + 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "501"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 4,
            "diagnostics": "",
            "bindings": [
              [
                "c",
                "501"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "b"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": "500"
        },
        {
          "name": "b_moved",
          "ty": "int",
          "value": "125"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "501"
        }
      ]
    }
  ]
});
