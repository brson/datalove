// Recorded by crates/datalove-doc-fixtures from replays/reactive-rerun-needs-rerun.world.
// Do not edit: run `BLESS=1 cargo test -p datalove-doc-fixtures`.
DataloveReplay.register("reactive-rerun-needs-rerun", {
  "version": 1,
  "steps": [
    {
      "input": "var b = 1",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Slot": {
                "name": "b",
                "ty": "int",
                "value": "1"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "1"
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
                "1"
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
          "value": "1"
        }
      ]
    },
    {
      "input": "var z = 10",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Slot": {
                "name": "z",
                "ty": "int",
                "value": "10"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "1"
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
                "1"
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
          "source": "var z = 10",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "z",
                  "ty": "int",
                  "value": "10"
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
                "z",
                "10"
              ]
            ],
            "provides": [
              "z"
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
          "value": "1"
        },
        {
          "name": "z",
          "ty": "int",
          "value": "10"
        }
      ]
    },
    {
      "input": "set b = 500\nset z = z + 1",
      "outcome": {
        "Unit": {
          "Ran": []
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "1"
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
          "source": "var z = 10",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "z",
                  "ty": "int",
                  "value": "10"
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
                "z",
                "11"
              ]
            ],
            "provides": [
              "z"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "set b = 500\nset z = z + 1",
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
            "overwrites": [
              "b"
            ],
            "updates": [
              "z"
            ],
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
          "name": "z",
          "ty": "int",
          "value": "11"
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
          "source": "var b = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "1"
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
          "source": "var z = 10",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "z",
                  "ty": "int",
                  "value": "10"
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
                "z",
                "11"
              ]
            ],
            "provides": [
              "z"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "set b = 500\nset z = z + 1",
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
            "overwrites": [
              "b"
            ],
            "updates": [
              "z"
            ],
            "revives": []
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
          "name": "c",
          "ty": "int",
          "value": "501"
        },
        {
          "name": "z",
          "ty": "int",
          "value": "11"
        }
      ]
    },
    {
      "input": "set b = 0",
      "outcome": {
        "Unit": {
          "Ran": []
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "var b = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "1"
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
                "0"
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
          "source": "var z = 10",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "z",
                  "ty": "int",
                  "value": "10"
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
                "z",
                "11"
              ]
            ],
            "provides": [
              "z"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "set b = 500\nset z = z + 1",
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
            "overwrites": [
              "b"
            ],
            "updates": [
              "z"
            ],
            "revives": []
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
          "ran": false,
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
        },
        {
          "number": 5,
          "expression": false,
          "source": "set b = 0",
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
            "overwrites": [
              "b"
            ],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": "0"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "501"
        },
        {
          "name": "z",
          "ty": "int",
          "value": "11"
        }
      ]
    },
    {
      "input": "/edit 4 let c = b + 2",
      "outcome": {
        "Edited": [
          {
            "unit": 2,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Slot": {
                    "name": "z",
                    "ty": "int",
                    "value": "10"
                  }
                }
              ]
            }
          },
          {
            "unit": 3,
            "compiled": false,
            "eval": {
              "Ran": []
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
                    "value": "502"
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
          "source": "var b = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "b",
                  "ty": "int",
                  "value": "1"
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
                "0"
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
          "source": "var z = 10",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "z",
                  "ty": "int",
                  "value": "10"
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
                "z",
                "11"
              ]
            ],
            "provides": [
              "z"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 3,
          "expression": false,
          "source": "set b = 500\nset z = z + 1",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 3,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [],
            "takes": [],
            "overwrites": [
              "b"
            ],
            "updates": [
              "z"
            ],
            "revives": []
          }
        },
        {
          "number": 4,
          "expression": false,
          "source": "let c = b + 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "c",
                  "ty": "int",
                  "value": "502"
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
                "502"
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
        },
        {
          "number": 5,
          "expression": false,
          "source": "set b = 0",
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
            "overwrites": [
              "b"
            ],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "b",
          "ty": "int",
          "value": "0"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "502"
        },
        {
          "name": "z",
          "ty": "int",
          "value": "11"
        }
      ]
    }
  ]
});
