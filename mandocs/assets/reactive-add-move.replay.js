// Recorded by crates/datalove-doc-fixtures from replays/reactive-add-move.world.
// Do not edit: run `BLESS=1 cargo test -p datalove-doc-fixtures`.
DataloveReplay.register("reactive-add-move", {
  "version": 1,
  "steps": [
    {
      "input": "let a = 1000",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "a",
                "ty": "int",
                "value": "1000"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "let a = 1000",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
                  "ty": "int",
                  "value": "1000"
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
                "a",
                "1000"
              ]
            ],
            "provides": [
              "a"
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
          "name": "a",
          "ty": "int",
          "value": "1000"
        }
      ]
    },
    {
      "input": "let c = a + 10",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "c",
                "ty": "int",
                "value": "1010"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "let a = 1000",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
                  "ty": "int",
                  "value": "1000"
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
                "a",
                "1000"
              ]
            ],
            "provides": [
              "a"
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
          "source": "let c = a + 10",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "1010"
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
                "c",
                "1010"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "a"
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
          "name": "a",
          "ty": "int",
          "value": "1000"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "1010"
        }
      ]
    },
    {
      "input": "let d = a + 1",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "d",
                "ty": "int",
                "value": "1001"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "let a = 1000",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
                  "ty": "int",
                  "value": "1000"
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
                "a",
                "1000"
              ]
            ],
            "provides": [
              "a"
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
          "source": "let c = a + 10",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "1010"
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
                "c",
                "1010"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "a"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "let d = a + 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "d",
                  "ty": "int",
                  "value": "1001"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [
              [
                "d",
                "1001"
              ]
            ],
            "provides": [
              "d"
            ],
            "reads": [
              "a"
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
          "name": "a",
          "ty": "int",
          "value": "1000"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "1010"
        },
        {
          "name": "d",
          "ty": "int",
          "value": "1001"
        }
      ]
    },
    {
      "input": "let e = a + 2",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "e",
                "ty": "int",
                "value": "1002"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "let a = 1000",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
                  "ty": "int",
                  "value": "1000"
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
                "a",
                "1000"
              ]
            ],
            "provides": [
              "a"
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
          "source": "let c = a + 10",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "1010"
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
                "c",
                "1010"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "a"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "let d = a + 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "d",
                  "ty": "int",
                  "value": "1001"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [
              [
                "d",
                "1001"
              ]
            ],
            "provides": [
              "d"
            ],
            "reads": [
              "a"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 4,
          "expression": false,
          "source": "let e = a + 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "e",
                  "ty": "int",
                  "value": "1002"
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
                "e",
                "1002"
              ]
            ],
            "provides": [
              "e"
            ],
            "reads": [
              "a"
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
          "name": "a",
          "ty": "int",
          "value": "1000"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "1010"
        },
        {
          "name": "d",
          "ty": "int",
          "value": "1001"
        },
        {
          "name": "e",
          "ty": "int",
          "value": "1002"
        }
      ]
    },
    {
      "input": "/edit 3 let d = a",
      "outcome": {
        "Edited": [
          {
            "unit": 3,
            "compiled": true,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "d",
                    "ty": "int",
                    "value": "1000"
                  }
                }
              ]
            }
          },
          {
            "unit": 4,
            "compiled": true,
            "eval": {
              "NotCompiled": "error[O013]: `a` was given away by an earlier unit\n  help: use `@` to clone `a` where it was given away"
            }
          }
        ]
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "let a = 1000",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
                  "ty": "int",
                  "value": "1000"
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
                "a",
                null
              ]
            ],
            "provides": [
              "a"
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
          "source": "let c = a + 10",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "1010"
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
                "c",
                "1010"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "a"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "let d = a",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "d",
                  "ty": "int",
                  "value": "1000"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [
              [
                "d",
                "1000"
              ]
            ],
            "provides": [
              "d"
            ],
            "reads": [],
            "takes": [
              "a"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 4,
          "expression": false,
          "source": "let e = a + 2",
          "status": "NotCompiled",
          "outcome": {
            "NotCompiled": "error[O013]: `a` was given away by an earlier unit\n  help: use `@` to clone `a` where it was given away"
          },
          "compiled": true,
          "ran": false,
          "detail": {
            "unit": 4,
            "diagnostics": "\u001b[31m[O013] Error:\u001b[0m `a` was given away by an earlier unit\n   \u001b[38;5;246m╭\u001b[0m\u001b[38;5;246m─\u001b[0m\u001b[38;5;246m[\u001b[0m unit 4:1:9 \u001b[38;5;246m]\u001b[0m\n   \u001b[38;5;246m│\u001b[0m\n \u001b[38;5;246m1 │\u001b[0m \u001b[38;5;249ml\u001b[0m\u001b[38;5;249me\u001b[0m\u001b[38;5;249mt\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249me\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m=\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;201ma\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m+\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m2\u001b[0m\n \u001b[38;5;240m  │\u001b[0m         \u001b[38;5;201m┬\u001b[0m  \n \u001b[38;5;240m  │\u001b[0m         \u001b[38;5;201m╰\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m used after it was given away\n \u001b[38;5;240m  │\u001b[0m \n \u001b[38;5;240m  │\u001b[0m \u001b[38;5;115mHelp\u001b[0m: use `@` to clone `a` where it was given away\n\u001b[38;5;246m───╯\u001b[0m\n",
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
          "name": "a",
          "ty": "int",
          "value": null
        },
        {
          "name": "c",
          "ty": "int",
          "value": "1010"
        },
        {
          "name": "d",
          "ty": "int",
          "value": "1000"
        }
      ]
    },
    {
      "input": "/edit 3 let d = a + 1",
      "outcome": {
        "Edited": [
          {
            "unit": 1,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "a",
                    "ty": "int",
                    "value": "1000"
                  }
                }
              ]
            }
          },
          {
            "unit": 3,
            "compiled": true,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "d",
                    "ty": "int",
                    "value": "1001"
                  }
                }
              ]
            }
          },
          {
            "unit": 4,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "e",
                    "ty": "int",
                    "value": "1002"
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
          "source": "let a = 1000",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
                  "ty": "int",
                  "value": "1000"
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
                "a",
                "1000"
              ]
            ],
            "provides": [
              "a"
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
          "source": "let c = a + 10",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "1010"
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
                "c",
                "1010"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "a"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "let d = a + 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "d",
                  "ty": "int",
                  "value": "1001"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [
              [
                "d",
                "1001"
              ]
            ],
            "provides": [
              "d"
            ],
            "reads": [
              "a"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 4,
          "expression": false,
          "source": "let e = a + 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "e",
                  "ty": "int",
                  "value": "1002"
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
                "e",
                "1002"
              ]
            ],
            "provides": [
              "e"
            ],
            "reads": [
              "a"
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
          "name": "a",
          "ty": "int",
          "value": "1000"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "1010"
        },
        {
          "name": "d",
          "ty": "int",
          "value": "1001"
        },
        {
          "name": "e",
          "ty": "int",
          "value": "1002"
        }
      ]
    }
  ]
});
