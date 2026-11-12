// Recorded by crates/datalove-doc-fixtures from replays/reactive-rerun-through-moves.world.
// Do not edit: run `BLESS=1 cargo test -p datalove-doc-fixtures`.
DataloveReplay.register("reactive-rerun-through-moves", {
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
      "input": "let a2 = a",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "a2",
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
          "source": "let a2 = a",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a2",
                  "ty": "int",
                  "value": "1000"
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
                "a2",
                "1000"
              ]
            ],
            "provides": [
              "a2"
            ],
            "reads": [],
            "takes": [
              "a"
            ],
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
          "name": "a2",
          "ty": "int",
          "value": "1000"
        }
      ]
    },
    {
      "input": "var w = a2",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Slot": {
                "name": "w",
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
          "source": "let a2 = a",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a2",
                  "ty": "int",
                  "value": "1000"
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
                "a2",
                null
              ]
            ],
            "provides": [
              "a2"
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
          "number": 3,
          "expression": false,
          "source": "var w = a2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "w",
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
                "w",
                "1000"
              ]
            ],
            "provides": [
              "w"
            ],
            "reads": [],
            "takes": [
              "a2"
            ],
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
          "name": "a2",
          "ty": "int",
          "value": null
        },
        {
          "name": "w",
          "ty": "int",
          "value": "1000"
        }
      ]
    },
    {
      "input": "let c = w + 1",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "c",
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
          "source": "let a2 = a",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a2",
                  "ty": "int",
                  "value": "1000"
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
                "a2",
                null
              ]
            ],
            "provides": [
              "a2"
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
          "number": 3,
          "expression": false,
          "source": "var w = a2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "w",
                  "ty": "int",
                  "value": "1000"
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
                "w",
                "1000"
              ]
            ],
            "provides": [
              "w"
            ],
            "reads": [],
            "takes": [
              "a2"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 4,
          "expression": false,
          "source": "let c = w + 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "1001"
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
                "1001"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "w"
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
          "value": null
        },
        {
          "name": "a2",
          "ty": "int",
          "value": null
        },
        {
          "name": "c",
          "ty": "int",
          "value": "1001"
        },
        {
          "name": "w",
          "ty": "int",
          "value": "1000"
        }
      ]
    },
    {
      "input": "set w = w * 2",
      "outcome": {
        "Unit": {
          "Ran": []
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
          "source": "let a2 = a",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a2",
                  "ty": "int",
                  "value": "1000"
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
                "a2",
                null
              ]
            ],
            "provides": [
              "a2"
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
          "number": 3,
          "expression": false,
          "source": "var w = a2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "w",
                  "ty": "int",
                  "value": "1000"
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
                "w",
                "2000"
              ]
            ],
            "provides": [
              "w"
            ],
            "reads": [],
            "takes": [
              "a2"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 4,
          "expression": false,
          "source": "let c = w + 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "1001"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 4,
            "diagnostics": "",
            "bindings": [
              [
                "c",
                "1001"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "w"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 5,
          "expression": false,
          "source": "set w = w * 2",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [
              "w"
            ],
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
          "name": "a2",
          "ty": "int",
          "value": null
        },
        {
          "name": "c",
          "ty": "int",
          "value": "1001"
        },
        {
          "name": "w",
          "ty": "int",
          "value": "2000"
        }
      ]
    },
    {
      "input": "/edit 4 let c = w + 2",
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
            "unit": 2,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "a2",
                    "ty": "int",
                    "value": "1000"
                  }
                }
              ]
            }
          },
          {
            "unit": 3,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Slot": {
                    "name": "w",
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
              "Ran": [
                {
                  "Value": {
                    "name": "c",
                    "ty": "int",
                    "value": "1002"
                  }
                }
              ]
            }
          },
          {
            "unit": 5,
            "compiled": false,
            "eval": {
              "Ran": []
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
          "source": "let a2 = a",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a2",
                  "ty": "int",
                  "value": "1000"
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
                "a2",
                null
              ]
            ],
            "provides": [
              "a2"
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
          "number": 3,
          "expression": false,
          "source": "var w = a2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "w",
                  "ty": "int",
                  "value": "1000"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [
              [
                "w",
                "2000"
              ]
            ],
            "provides": [
              "w"
            ],
            "reads": [],
            "takes": [
              "a2"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 4,
          "expression": false,
          "source": "let c = w + 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
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
                "c",
                "1002"
              ]
            ],
            "provides": [
              "c"
            ],
            "reads": [
              "w"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 5,
          "expression": false,
          "source": "set w = w * 2",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [
              "w"
            ],
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
          "name": "a2",
          "ty": "int",
          "value": null
        },
        {
          "name": "c",
          "ty": "int",
          "value": "1002"
        },
        {
          "name": "w",
          "ty": "int",
          "value": "2000"
        }
      ]
    }
  ]
});
