// Recorded by crates/datalove-doc-fixtures from replays/reactive-four-accesses.world.
// Do not edit: run `BLESS=1 cargo test -p datalove-doc-fixtures`.
DataloveReplay.register("reactive-four-accesses", {
  "version": 1,
  "steps": [
    {
      "input": "let a = 1",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "a",
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
          "source": "let a = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
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
                "a",
                "1"
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
          "value": "1"
        }
      ]
    },
    {
      "input": "let b = 2",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "b",
                "ty": "int",
                "value": "2"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "let a = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
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
                "a",
                "1"
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
          "source": "let b = 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b",
                  "ty": "int",
                  "value": "2"
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
                "b",
                "2"
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
          "name": "a",
          "ty": "int",
          "value": "1"
        },
        {
          "name": "b",
          "ty": "int",
          "value": "2"
        }
      ]
    },
    {
      "input": "var c = 3",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Slot": {
                "name": "c",
                "ty": "int",
                "value": "3"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "let a = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
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
                "a",
                "1"
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
          "source": "let b = 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b",
                  "ty": "int",
                  "value": "2"
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
                "b",
                "2"
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
          "number": 3,
          "expression": false,
          "source": "var c = 3",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "c",
                  "ty": "int",
                  "value": "3"
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
                "c",
                "3"
              ]
            ],
            "provides": [
              "c"
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
          "value": "1"
        },
        {
          "name": "b",
          "ty": "int",
          "value": "2"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "3"
        }
      ]
    },
    {
      "input": "var d = 4",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Slot": {
                "name": "d",
                "ty": "int",
                "value": "4"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "let a = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
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
                "a",
                "1"
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
          "source": "let b = 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b",
                  "ty": "int",
                  "value": "2"
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
                "b",
                "2"
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
          "number": 3,
          "expression": false,
          "source": "var c = 3",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "c",
                  "ty": "int",
                  "value": "3"
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
                "c",
                "3"
              ]
            ],
            "provides": [
              "c"
            ],
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
          "source": "var d = 4",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "d",
                  "ty": "int",
                  "value": "4"
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
                "d",
                "4"
              ]
            ],
            "provides": [
              "d"
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
          "value": "1"
        },
        {
          "name": "b",
          "ty": "int",
          "value": "2"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "3"
        },
        {
          "name": "d",
          "ty": "int",
          "value": "4"
        }
      ]
    },
    {
      "input": "let e = a + 10\nlet f = b\nset c = 30\nset d = d * 2",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "e",
                "ty": "int",
                "value": "11"
              }
            },
            {
              "Value": {
                "name": "f",
                "ty": "int",
                "value": "2"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "let a = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
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
                "a",
                "1"
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
          "source": "let b = 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b",
                  "ty": "int",
                  "value": "2"
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
          "number": 3,
          "expression": false,
          "source": "var c = 3",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "c",
                  "ty": "int",
                  "value": "3"
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
                "c",
                "30"
              ]
            ],
            "provides": [
              "c"
            ],
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
          "source": "var d = 4",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "d",
                  "ty": "int",
                  "value": "4"
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
                "d",
                "8"
              ]
            ],
            "provides": [
              "d"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 5,
          "expression": false,
          "source": "let e = a + 10\nlet f = b\nset c = 30\nset d = d * 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "e",
                  "ty": "int",
                  "value": "11"
                }
              },
              {
                "Value": {
                  "name": "f",
                  "ty": "int",
                  "value": "2"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [
              [
                "e",
                "11"
              ],
              [
                "f",
                "2"
              ]
            ],
            "provides": [
              "e",
              "f"
            ],
            "reads": [
              "a"
            ],
            "takes": [
              "b"
            ],
            "overwrites": [
              "c"
            ],
            "updates": [
              "d"
            ],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "a",
          "ty": "int",
          "value": "1"
        },
        {
          "name": "b",
          "ty": "int",
          "value": null
        },
        {
          "name": "c",
          "ty": "int",
          "value": "30"
        },
        {
          "name": "d",
          "ty": "int",
          "value": "8"
        },
        {
          "name": "e",
          "ty": "int",
          "value": "11"
        },
        {
          "name": "f",
          "ty": "int",
          "value": "2"
        }
      ]
    },
    {
      "input": "/edit 5 let e = a + 100\nlet f = b\nset c = 30\nset d = d * 2",
      "outcome": {
        "Edited": [
          {
            "unit": 2,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "b",
                    "ty": "int",
                    "value": "2"
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
                  "Slot": {
                    "name": "d",
                    "ty": "int",
                    "value": "4"
                  }
                }
              ]
            }
          },
          {
            "unit": 5,
            "compiled": true,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "e",
                    "ty": "int",
                    "value": "101"
                  }
                },
                {
                  "Value": {
                    "name": "f",
                    "ty": "int",
                    "value": "2"
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
          "source": "let a = 1",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
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
                "a",
                "1"
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
          "source": "let b = 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b",
                  "ty": "int",
                  "value": "2"
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
          "number": 3,
          "expression": false,
          "source": "var c = 3",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "c",
                  "ty": "int",
                  "value": "3"
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
                "c",
                "30"
              ]
            ],
            "provides": [
              "c"
            ],
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
          "source": "var d = 4",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "d",
                  "ty": "int",
                  "value": "4"
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
                "d",
                "8"
              ]
            ],
            "provides": [
              "d"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 5,
          "expression": false,
          "source": "let e = a + 100\nlet f = b\nset c = 30\nset d = d * 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "e",
                  "ty": "int",
                  "value": "101"
                }
              },
              {
                "Value": {
                  "name": "f",
                  "ty": "int",
                  "value": "2"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [
              [
                "e",
                "101"
              ],
              [
                "f",
                "2"
              ]
            ],
            "provides": [
              "e",
              "f"
            ],
            "reads": [
              "a"
            ],
            "takes": [
              "b"
            ],
            "overwrites": [
              "c"
            ],
            "updates": [
              "d"
            ],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "a",
          "ty": "int",
          "value": "1"
        },
        {
          "name": "b",
          "ty": "int",
          "value": null
        },
        {
          "name": "c",
          "ty": "int",
          "value": "30"
        },
        {
          "name": "d",
          "ty": "int",
          "value": "8"
        },
        {
          "name": "e",
          "ty": "int",
          "value": "101"
        },
        {
          "name": "f",
          "ty": "int",
          "value": "2"
        }
      ]
    },
    {
      "input": "/edit 1 let a = 2",
      "outcome": {
        "Edited": [
          {
            "unit": 1,
            "compiled": true,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "a",
                    "ty": "int",
                    "value": "2"
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
                    "name": "b",
                    "ty": "int",
                    "value": "2"
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
                  "Slot": {
                    "name": "d",
                    "ty": "int",
                    "value": "4"
                  }
                }
              ]
            }
          },
          {
            "unit": 5,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "e",
                    "ty": "int",
                    "value": "102"
                  }
                },
                {
                  "Value": {
                    "name": "f",
                    "ty": "int",
                    "value": "2"
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
          "source": "let a = 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
                  "ty": "int",
                  "value": "2"
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
                "2"
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
          "source": "let b = 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b",
                  "ty": "int",
                  "value": "2"
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
          "number": 3,
          "expression": false,
          "source": "var c = 3",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "c",
                  "ty": "int",
                  "value": "3"
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
                "c",
                "30"
              ]
            ],
            "provides": [
              "c"
            ],
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
          "source": "var d = 4",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "d",
                  "ty": "int",
                  "value": "4"
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
                "d",
                "8"
              ]
            ],
            "provides": [
              "d"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 5,
          "expression": false,
          "source": "let e = a + 100\nlet f = b\nset c = 30\nset d = d * 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "e",
                  "ty": "int",
                  "value": "102"
                }
              },
              {
                "Value": {
                  "name": "f",
                  "ty": "int",
                  "value": "2"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [
              [
                "e",
                "102"
              ],
              [
                "f",
                "2"
              ]
            ],
            "provides": [
              "e",
              "f"
            ],
            "reads": [
              "a"
            ],
            "takes": [
              "b"
            ],
            "overwrites": [
              "c"
            ],
            "updates": [
              "d"
            ],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "a",
          "ty": "int",
          "value": "2"
        },
        {
          "name": "b",
          "ty": "int",
          "value": null
        },
        {
          "name": "c",
          "ty": "int",
          "value": "30"
        },
        {
          "name": "d",
          "ty": "int",
          "value": "8"
        },
        {
          "name": "e",
          "ty": "int",
          "value": "102"
        },
        {
          "name": "f",
          "ty": "int",
          "value": "2"
        }
      ]
    },
    {
      "input": "/edit 5 let e = a + 100\nlet f = b\nset c = 30\nset d = d * 2\nlet x = y",
      "outcome": {
        "Edited": [
          {
            "unit": 2,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "b",
                    "ty": "int",
                    "value": "2"
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
                    "name": "c",
                    "ty": "int",
                    "value": "3"
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
                  "Slot": {
                    "name": "d",
                    "ty": "int",
                    "value": "4"
                  }
                }
              ]
            }
          },
          {
            "unit": 5,
            "compiled": true,
            "eval": {
              "NotCompiled": "cannot find `y` in this scope"
            }
          }
        ]
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "let a = 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "a",
                  "ty": "int",
                  "value": "2"
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
                "2"
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
          "source": "let b = 2",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "b",
                  "ty": "int",
                  "value": "2"
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
                "b",
                "2"
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
          "number": 3,
          "expression": false,
          "source": "var c = 3",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "c",
                  "ty": "int",
                  "value": "3"
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
                "c",
                "3"
              ]
            ],
            "provides": [
              "c"
            ],
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
          "source": "var d = 4",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "d",
                  "ty": "int",
                  "value": "4"
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
                "d",
                "4"
              ]
            ],
            "provides": [
              "d"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 5,
          "expression": false,
          "source": "let e = a + 100\nlet f = b\nset c = 30\nset d = d * 2\nlet x = y",
          "status": "NotCompiled",
          "outcome": {
            "NotCompiled": "cannot find `y` in this scope"
          },
          "compiled": true,
          "ran": false,
          "detail": {
            "unit": 5,
            "diagnostics": "\u001b[31m[F001] Error:\u001b[0m cannot find value `y` in this scope\n   \u001b[38;5;246m╭\u001b[0m\u001b[38;5;246m─\u001b[0m\u001b[38;5;246m[\u001b[0m unit 5:5:9 \u001b[38;5;246m]\u001b[0m\n   \u001b[38;5;246m│\u001b[0m\n \u001b[38;5;246m5 │\u001b[0m \u001b[38;5;249ml\u001b[0m\u001b[38;5;249me\u001b[0m\u001b[38;5;249mt\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249mx\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m=\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;201my\u001b[0m\n \u001b[38;5;240m  │\u001b[0m         \u001b[38;5;201m┬\u001b[0m  \n \u001b[38;5;240m  │\u001b[0m         \u001b[38;5;201m╰\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m not found in this scope\n\u001b[38;5;246m───╯\u001b[0m\n",
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
          "value": "2"
        },
        {
          "name": "b",
          "ty": "int",
          "value": "2"
        },
        {
          "name": "c",
          "ty": "int",
          "value": "3"
        },
        {
          "name": "d",
          "ty": "int",
          "value": "4"
        }
      ]
    }
  ]
});
