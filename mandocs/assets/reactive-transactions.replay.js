// Recorded by crates/datalove-doc-fixtures from replays/reactive-transactions.world.
// Do not edit: run `BLESS=1 cargo test -p datalove-doc-fixtures`.
DataloveReplay.register("reactive-transactions", {
  "version": 1,
  "steps": [
    {
      "input": "require module sys/std/list",
      "outcome": {
        "Unit": {
          "Ran": []
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": []
    },
    {
      "input": "require module sys/std/map",
      "outcome": {
        "Unit": {
          "Ran": []
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": []
    },
    {
      "input": "require module sys/std/string",
      "outcome": {
        "Unit": {
          "Ran": []
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": []
    },
    {
      "input": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "transactions",
                "ty": "[{ day: u32, payee: string, cents: int }]",
                "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    },
    {
      "input": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Function": {
                "name": "category"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "source": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Function": {
                  "name": "category"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "category"
            ],
            "reads": [
              "category",
              "payee",
              "string",
              "string"
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
          "name": "category",
          "ty": "fun(string): string",
          "value": "-"
        },
        {
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    },
    {
      "input": "let budget: %{string = int} = %{\n  \"groceries\" = 25000,\n  \"dining\" = 15000,\n  \"car\" = 10000,\n  \"bills\" = 15000,\n  \"other\" = 5000\n}",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "budget",
                "ty": "%{string = int}",
                "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "source": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Function": {
                  "name": "category"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "category"
            ],
            "reads": [
              "category",
              "payee",
              "string",
              "string"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 6,
          "expression": false,
          "source": "let budget: %{string = int} = %{\n  \"groceries\" = 25000,\n  \"dining\" = 15000,\n  \"car\" = 10000,\n  \"bills\" = 15000,\n  \"other\" = 5000\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "budget",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 6,
            "diagnostics": "",
            "bindings": [
              [
                "budget",
                "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
              ]
            ],
            "provides": [
              "budget"
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
          "name": "budget",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
        },
        {
          "name": "category",
          "ty": "fun(string): string",
          "value": "-"
        },
        {
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    },
    {
      "input": "var spent: %{string = int} = %{}\nfor t in ref transactions\n  let c = category(ref t.payee)\n  let so_far = map.get_or(ref spent, ref c, 0)\n  call map.insert(mut spent, c, so_far + t.cents)\nend for",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Slot": {
                "name": "spent",
                "ty": "%{string = int}",
                "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "source": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Function": {
                  "name": "category"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "category"
            ],
            "reads": [
              "category",
              "payee",
              "string",
              "string"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 6,
          "expression": false,
          "source": "let budget: %{string = int} = %{\n  \"groceries\" = 25000,\n  \"dining\" = 15000,\n  \"car\" = 10000,\n  \"bills\" = 15000,\n  \"other\" = 5000\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "budget",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 6,
            "diagnostics": "",
            "bindings": [
              [
                "budget",
                "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
              ]
            ],
            "provides": [
              "budget"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 7,
          "expression": false,
          "source": "var spent: %{string = int} = %{}\nfor t in ref transactions\n  let c = category(ref t.payee)\n  let so_far = map.get_or(ref spent, ref c, 0)\n  call map.insert(mut spent, c, so_far + t.cents)\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "spent",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 7,
            "diagnostics": "",
            "bindings": [
              [
                "spent",
                "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}"
              ]
            ],
            "provides": [
              "spent"
            ],
            "reads": [
              "c",
              "category",
              "so_far",
              "spent",
              "t",
              "transactions",
              "map",
              "map"
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
          "name": "budget",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
        },
        {
          "name": "category",
          "ty": "fun(string): string",
          "value": "-"
        },
        {
          "name": "spent",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}"
        },
        {
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    },
    {
      "input": "var over: [string] = []\nfor c, amount in ref spent\n  if amount .> budget[c]!\n    call list.push(mut over, c@)\n  end if\nend for",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Slot": {
                "name": "over",
                "ty": "[string]",
                "value": "[]"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "source": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Function": {
                  "name": "category"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "category"
            ],
            "reads": [
              "category",
              "payee",
              "string",
              "string"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 6,
          "expression": false,
          "source": "let budget: %{string = int} = %{\n  \"groceries\" = 25000,\n  \"dining\" = 15000,\n  \"car\" = 10000,\n  \"bills\" = 15000,\n  \"other\" = 5000\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "budget",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 6,
            "diagnostics": "",
            "bindings": [
              [
                "budget",
                "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
              ]
            ],
            "provides": [
              "budget"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 7,
          "expression": false,
          "source": "var spent: %{string = int} = %{}\nfor t in ref transactions\n  let c = category(ref t.payee)\n  let so_far = map.get_or(ref spent, ref c, 0)\n  call map.insert(mut spent, c, so_far + t.cents)\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "spent",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 7,
            "diagnostics": "",
            "bindings": [
              [
                "spent",
                "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}"
              ]
            ],
            "provides": [
              "spent"
            ],
            "reads": [
              "c",
              "category",
              "so_far",
              "spent",
              "t",
              "transactions",
              "map",
              "map"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 8,
          "expression": false,
          "source": "var over: [string] = []\nfor c, amount in ref spent\n  if amount .> budget[c]!\n    call list.push(mut over, c@)\n  end if\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "over",
                  "ty": "[string]",
                  "value": "[]"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 8,
            "diagnostics": "",
            "bindings": [
              [
                "over",
                "[]"
              ]
            ],
            "provides": [
              "over"
            ],
            "reads": [
              "amount",
              "budget",
              "c",
              "over",
              "spent",
              "list"
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
          "name": "budget",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
        },
        {
          "name": "category",
          "ty": "fun(string): string",
          "value": "-"
        },
        {
          "name": "over",
          "ty": "[string]",
          "value": "[]"
        },
        {
          "name": "spent",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}"
        },
        {
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    },
    {
      "input": "let output = {\n  spent = spent,\n  over = over\n}",
      "outcome": {
        "Unit": {
          "Ran": [
            {
              "Value": {
                "name": "output",
                "ty": "{ spent: %{string = int}, over: [string] }",
                "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}, over = []}"
              }
            }
          ]
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "source": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Function": {
                  "name": "category"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "category"
            ],
            "reads": [
              "category",
              "payee",
              "string",
              "string"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 6,
          "expression": false,
          "source": "let budget: %{string = int} = %{\n  \"groceries\" = 25000,\n  \"dining\" = 15000,\n  \"car\" = 10000,\n  \"bills\" = 15000,\n  \"other\" = 5000\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "budget",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 6,
            "diagnostics": "",
            "bindings": [
              [
                "budget",
                "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
              ]
            ],
            "provides": [
              "budget"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 7,
          "expression": false,
          "source": "var spent: %{string = int} = %{}\nfor t in ref transactions\n  let c = category(ref t.payee)\n  let so_far = map.get_or(ref spent, ref c, 0)\n  call map.insert(mut spent, c, so_far + t.cents)\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "spent",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 7,
            "diagnostics": "",
            "bindings": [
              [
                "spent",
                null
              ]
            ],
            "provides": [
              "spent"
            ],
            "reads": [
              "c",
              "category",
              "so_far",
              "spent",
              "t",
              "transactions",
              "map",
              "map"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 8,
          "expression": false,
          "source": "var over: [string] = []\nfor c, amount in ref spent\n  if amount .> budget[c]!\n    call list.push(mut over, c@)\n  end if\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "over",
                  "ty": "[string]",
                  "value": "[]"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 8,
            "diagnostics": "",
            "bindings": [
              [
                "over",
                null
              ]
            ],
            "provides": [
              "over"
            ],
            "reads": [
              "amount",
              "budget",
              "c",
              "over",
              "spent",
              "list"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 9,
          "expression": false,
          "source": "let output = {\n  spent = spent,\n  over = over\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "output",
                  "ty": "{ spent: %{string = int}, over: [string] }",
                  "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}, over = []}"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 9,
            "diagnostics": "",
            "bindings": [
              [
                "output",
                "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}, over = []}"
              ]
            ],
            "provides": [
              "output"
            ],
            "reads": [],
            "takes": [
              "over",
              "spent"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "budget",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
        },
        {
          "name": "category",
          "ty": "fun(string): string",
          "value": "-"
        },
        {
          "name": "output",
          "ty": "{ spent: %{string = int}, over: [string] }",
          "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 23819, \"other\" = 3315}, over = []}"
        },
        {
          "name": "over",
          "ty": "[string]",
          "value": null
        },
        {
          "name": "spent",
          "ty": "%{string = int}",
          "value": null
        },
        {
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    },
    {
      "input": "/edit 5 fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n    or string.starts_with(ref payee, ref \"BI-RITE\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
      "outcome": {
        "Edited": [
          {
            "unit": 5,
            "compiled": true,
            "eval": {
              "Ran": [
                {
                  "Function": {
                    "name": "category"
                  }
                }
              ]
            }
          },
          {
            "unit": 7,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Slot": {
                    "name": "spent",
                    "ty": "%{string = int}",
                    "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
                  }
                }
              ]
            }
          },
          {
            "unit": 8,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Slot": {
                    "name": "over",
                    "ty": "[string]",
                    "value": "[\"groceries\"]"
                  }
                }
              ]
            }
          },
          {
            "unit": 9,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "output",
                    "ty": "{ spent: %{string = int}, over: [string] }",
                    "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = [\"groceries\"]}"
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
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "source": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n    or string.starts_with(ref payee, ref \"BI-RITE\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Function": {
                  "name": "category"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "category"
            ],
            "reads": [
              "category",
              "payee",
              "string",
              "string"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 6,
          "expression": false,
          "source": "let budget: %{string = int} = %{\n  \"groceries\" = 25000,\n  \"dining\" = 15000,\n  \"car\" = 10000,\n  \"bills\" = 15000,\n  \"other\" = 5000\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "budget",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 6,
            "diagnostics": "",
            "bindings": [
              [
                "budget",
                "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
              ]
            ],
            "provides": [
              "budget"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 7,
          "expression": false,
          "source": "var spent: %{string = int} = %{}\nfor t in ref transactions\n  let c = category(ref t.payee)\n  let so_far = map.get_or(ref spent, ref c, 0)\n  call map.insert(mut spent, c, so_far + t.cents)\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "spent",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 7,
            "diagnostics": "",
            "bindings": [
              [
                "spent",
                null
              ]
            ],
            "provides": [
              "spent"
            ],
            "reads": [
              "c",
              "category",
              "so_far",
              "spent",
              "t",
              "transactions",
              "map",
              "map"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 8,
          "expression": false,
          "source": "var over: [string] = []\nfor c, amount in ref spent\n  if amount .> budget[c]!\n    call list.push(mut over, c@)\n  end if\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "over",
                  "ty": "[string]",
                  "value": "[\"groceries\"]"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 8,
            "diagnostics": "",
            "bindings": [
              [
                "over",
                null
              ]
            ],
            "provides": [
              "over"
            ],
            "reads": [
              "amount",
              "budget",
              "c",
              "over",
              "spent",
              "list"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 9,
          "expression": false,
          "source": "let output = {\n  spent = spent,\n  over = over\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "output",
                  "ty": "{ spent: %{string = int}, over: [string] }",
                  "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = [\"groceries\"]}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 9,
            "diagnostics": "",
            "bindings": [
              [
                "output",
                "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = [\"groceries\"]}"
              ]
            ],
            "provides": [
              "output"
            ],
            "reads": [],
            "takes": [
              "over",
              "spent"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "budget",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 25000, \"other\" = 5000}"
        },
        {
          "name": "category",
          "ty": "fun(string): string",
          "value": "-"
        },
        {
          "name": "output",
          "ty": "{ spent: %{string = int}, over: [string] }",
          "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = [\"groceries\"]}"
        },
        {
          "name": "over",
          "ty": "[string]",
          "value": null
        },
        {
          "name": "spent",
          "ty": "%{string = int}",
          "value": null
        },
        {
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    },
    {
      "input": "/edit 6 let budget: %{string = int} = %{\n  \"groceries\" = 30000,\n  \"dining\" = 15000,\n  \"bills\" = 15000\n}",
      "outcome": {
        "Edited": [
          {
            "unit": 6,
            "compiled": true,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "budget",
                    "ty": "%{string = int}",
                    "value": "%{\"bills\" = 15000, \"dining\" = 15000, \"groceries\" = 30000}"
                  }
                }
              ]
            }
          },
          {
            "unit": 7,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Slot": {
                    "name": "spent",
                    "ty": "%{string = int}",
                    "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
                  }
                }
              ]
            }
          },
          {
            "unit": 8,
            "compiled": false,
            "eval": {
              "ReturnedEarly": "er error \"key not found\""
            }
          },
          {
            "unit": 9,
            "compiled": false,
            "eval": {
              "Blocked": {
                "on": [
                  8
                ]
              }
            }
          }
        ]
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "source": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n    or string.starts_with(ref payee, ref \"BI-RITE\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Function": {
                  "name": "category"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "category"
            ],
            "reads": [
              "category",
              "payee",
              "string",
              "string"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 6,
          "expression": false,
          "source": "let budget: %{string = int} = %{\n  \"groceries\" = 30000,\n  \"dining\" = 15000,\n  \"bills\" = 15000\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "budget",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 15000, \"dining\" = 15000, \"groceries\" = 30000}"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 6,
            "diagnostics": "",
            "bindings": [
              [
                "budget",
                "%{\"bills\" = 15000, \"dining\" = 15000, \"groceries\" = 30000}"
              ]
            ],
            "provides": [
              "budget"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 7,
          "expression": false,
          "source": "var spent: %{string = int} = %{}\nfor t in ref transactions\n  let c = category(ref t.payee)\n  let so_far = map.get_or(ref spent, ref c, 0)\n  call map.insert(mut spent, c, so_far + t.cents)\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "spent",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 7,
            "diagnostics": "",
            "bindings": [
              [
                "spent",
                "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
              ]
            ],
            "provides": [
              "spent"
            ],
            "reads": [
              "c",
              "category",
              "so_far",
              "spent",
              "t",
              "transactions",
              "map",
              "map"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 8,
          "expression": false,
          "source": "var over: [string] = []\nfor c, amount in ref spent\n  if amount .> budget[c]!\n    call list.push(mut over, c@)\n  end if\nend for",
          "status": "ReturnedEarly",
          "outcome": {
            "ReturnedEarly": "er error \"key not found\""
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 8,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "over"
            ],
            "reads": [
              "amount",
              "budget",
              "c",
              "over",
              "spent",
              "list"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 9,
          "expression": false,
          "source": "let output = {\n  spent = spent,\n  over = over\n}",
          "status": "Blocked",
          "outcome": {
            "Blocked": {
              "on": [
                8
              ]
            }
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 9,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "output"
            ],
            "reads": [],
            "takes": [
              "over",
              "spent"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "budget",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 15000, \"dining\" = 15000, \"groceries\" = 30000}"
        },
        {
          "name": "category",
          "ty": "fun(string): string",
          "value": "-"
        },
        {
          "name": "spent",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
        },
        {
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    },
    {
      "input": "/edit 6 let budget: %{string = int} = %{\n  \"groceries\" = 30000,\n  \"dining\" = 15000,\n  \"car\" = \"lots\",\n  \"bills\" = 15000\n}",
      "outcome": {
        "Edited": [
          {
            "unit": 6,
            "compiled": true,
            "eval": {
              "NotCompiled": "mismatched types: expected `int`, found `string`"
            }
          },
          {
            "unit": 7,
            "compiled": false,
            "eval": {
              "Ran": [
                {
                  "Slot": {
                    "name": "spent",
                    "ty": "%{string = int}",
                    "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
                  }
                }
              ]
            }
          },
          {
            "unit": 8,
            "compiled": true,
            "eval": {
              "NotCompiled": "cannot find `budget` in this scope"
            }
          },
          {
            "unit": 9,
            "compiled": true,
            "eval": {
              "NotCompiled": "cannot find `over` in this scope"
            }
          }
        ]
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "source": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n    or string.starts_with(ref payee, ref \"BI-RITE\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Function": {
                  "name": "category"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "category"
            ],
            "reads": [
              "category",
              "payee",
              "string",
              "string"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 6,
          "expression": false,
          "source": "let budget: %{string = int} = %{\n  \"groceries\" = 30000,\n  \"dining\" = 15000,\n  \"car\" = \"lots\",\n  \"bills\" = 15000\n}",
          "status": "NotCompiled",
          "outcome": {
            "NotCompiled": "mismatched types: expected `int`, found `string`"
          },
          "compiled": true,
          "ran": false,
          "detail": {
            "unit": 6,
            "diagnostics": "\u001b[31m[F016] Error:\u001b[0m mismatched types: expected `int`, found `string`\n   \u001b[38;5;246m╭\u001b[0m\u001b[38;5;246m─\u001b[0m\u001b[38;5;246m[\u001b[0m unit 6:4:11 \u001b[38;5;246m]\u001b[0m\n   \u001b[38;5;246m│\u001b[0m\n \u001b[38;5;246m4 │\u001b[0m \u001b[38;5;249m \u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m\"\u001b[0m\u001b[38;5;249mc\u001b[0m\u001b[38;5;249ma\u001b[0m\u001b[38;5;249mr\u001b[0m\u001b[38;5;249m\"\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m=\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;201m\"\u001b[0m\u001b[38;5;201ml\u001b[0m\u001b[38;5;201mo\u001b[0m\u001b[38;5;201mt\u001b[0m\u001b[38;5;201ms\u001b[0m\u001b[38;5;201m\"\u001b[0m\u001b[38;5;249m,\u001b[0m\n \u001b[38;5;240m  │\u001b[0m           \u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m┬\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m  \n \u001b[38;5;240m  │\u001b[0m              \u001b[38;5;201m╰\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m type mismatch\n\u001b[38;5;246m───╯\u001b[0m\n",
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
          "number": 7,
          "expression": false,
          "source": "var spent: %{string = int} = %{}\nfor t in ref transactions\n  let c = category(ref t.payee)\n  let so_far = map.get_or(ref spent, ref c, 0)\n  call map.insert(mut spent, c, so_far + t.cents)\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "spent",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": true,
          "detail": {
            "unit": 7,
            "diagnostics": "",
            "bindings": [
              [
                "spent",
                "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
              ]
            ],
            "provides": [
              "spent"
            ],
            "reads": [
              "c",
              "category",
              "so_far",
              "spent",
              "t",
              "transactions",
              "map",
              "map"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 8,
          "expression": false,
          "source": "var over: [string] = []\nfor c, amount in ref spent\n  if amount .> budget[c]!\n    call list.push(mut over, c@)\n  end if\nend for",
          "status": "NotCompiled",
          "outcome": {
            "NotCompiled": "cannot find `budget` in this scope"
          },
          "compiled": true,
          "ran": false,
          "detail": {
            "unit": 8,
            "diagnostics": "\u001b[31m[F001] Error:\u001b[0m cannot find value `budget` in this scope\n   \u001b[38;5;246m╭\u001b[0m\u001b[38;5;246m─\u001b[0m\u001b[38;5;246m[\u001b[0m unit 8:3:22 \u001b[38;5;246m]\u001b[0m\n   \u001b[38;5;246m│\u001b[0m\n \u001b[38;5;246m3 │\u001b[0m \u001b[38;5;249m \u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249mi\u001b[0m\u001b[38;5;249mf\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249ma\u001b[0m\u001b[38;5;249mm\u001b[0m\u001b[38;5;249mo\u001b[0m\u001b[38;5;249mu\u001b[0m\u001b[38;5;249mn\u001b[0m\u001b[38;5;249mt\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m.\u001b[0m\u001b[38;5;249m>\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249mb\u001b[0m\u001b[38;5;249mu\u001b[0m\u001b[38;5;249md\u001b[0m\u001b[38;5;249mg\u001b[0m\u001b[38;5;249me\u001b[0m\u001b[38;5;249mt\u001b[0m\u001b[38;5;201m[\u001b[0m\u001b[38;5;201mc\u001b[0m\u001b[38;5;201m]\u001b[0m\u001b[38;5;201m!\u001b[0m\n \u001b[38;5;240m  │\u001b[0m                      \u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m┬\u001b[0m\u001b[38;5;201m─\u001b[0m  \n \u001b[38;5;240m  │\u001b[0m                        \u001b[38;5;201m╰\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m not found in this scope\n\u001b[38;5;246m───╯\u001b[0m\n",
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
          "number": 9,
          "expression": false,
          "source": "let output = {\n  spent = spent,\n  over = over\n}",
          "status": "NotCompiled",
          "outcome": {
            "NotCompiled": "cannot find `over` in this scope"
          },
          "compiled": true,
          "ran": false,
          "detail": {
            "unit": 9,
            "diagnostics": "\u001b[31m[F001] Error:\u001b[0m cannot find value `over` in this scope\n   \u001b[38;5;246m╭\u001b[0m\u001b[38;5;246m─\u001b[0m\u001b[38;5;246m[\u001b[0m unit 9:3:10 \u001b[38;5;246m]\u001b[0m\n   \u001b[38;5;246m│\u001b[0m\n \u001b[38;5;246m3 │\u001b[0m \u001b[38;5;249m \u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249mo\u001b[0m\u001b[38;5;249mv\u001b[0m\u001b[38;5;249me\u001b[0m\u001b[38;5;249mr\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;249m=\u001b[0m\u001b[38;5;249m \u001b[0m\u001b[38;5;201mo\u001b[0m\u001b[38;5;201mv\u001b[0m\u001b[38;5;201me\u001b[0m\u001b[38;5;201mr\u001b[0m\n \u001b[38;5;240m  │\u001b[0m          \u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m┬\u001b[0m\u001b[38;5;201m─\u001b[0m  \n \u001b[38;5;240m  │\u001b[0m            \u001b[38;5;201m╰\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m\u001b[38;5;201m─\u001b[0m not found in this scope\n\u001b[38;5;246m───╯\u001b[0m\n",
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
          "name": "category",
          "ty": "fun(string): string",
          "value": "-"
        },
        {
          "name": "spent",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
        },
        {
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    },
    {
      "input": "/edit 6 let budget: %{string = int} = %{\n  \"groceries\" = 30000,\n  \"dining\" = 15000,\n  \"car\" = 10000,\n  \"bills\" = 15000\n}",
      "outcome": {
        "Edited": [
          {
            "unit": 6,
            "compiled": true,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "budget",
                    "ty": "%{string = int}",
                    "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 30000}"
                  }
                }
              ]
            }
          },
          {
            "unit": 8,
            "compiled": true,
            "eval": {
              "Ran": [
                {
                  "Slot": {
                    "name": "over",
                    "ty": "[string]",
                    "value": "[]"
                  }
                }
              ]
            }
          },
          {
            "unit": 9,
            "compiled": true,
            "eval": {
              "Ran": [
                {
                  "Value": {
                    "name": "output",
                    "ty": "{ spent: %{string = int}, over: [string] }",
                    "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = []}"
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
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "source": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n    or string.starts_with(ref payee, ref \"BI-RITE\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Function": {
                  "name": "category"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "category"
            ],
            "reads": [
              "category",
              "payee",
              "string",
              "string"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 6,
          "expression": false,
          "source": "let budget: %{string = int} = %{\n  \"groceries\" = 30000,\n  \"dining\" = 15000,\n  \"car\" = 10000,\n  \"bills\" = 15000\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "budget",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 30000}"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 6,
            "diagnostics": "",
            "bindings": [
              [
                "budget",
                "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 30000}"
              ]
            ],
            "provides": [
              "budget"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 7,
          "expression": false,
          "source": "var spent: %{string = int} = %{}\nfor t in ref transactions\n  let c = category(ref t.payee)\n  let so_far = map.get_or(ref spent, ref c, 0)\n  call map.insert(mut spent, c, so_far + t.cents)\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "spent",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 7,
            "diagnostics": "",
            "bindings": [
              [
                "spent",
                null
              ]
            ],
            "provides": [
              "spent"
            ],
            "reads": [
              "c",
              "category",
              "so_far",
              "spent",
              "t",
              "transactions",
              "map",
              "map"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 8,
          "expression": false,
          "source": "var over: [string] = []\nfor c, amount in ref spent\n  if amount .> budget[c]!\n    call list.push(mut over, c@)\n  end if\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "over",
                  "ty": "[string]",
                  "value": "[]"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 8,
            "diagnostics": "",
            "bindings": [
              [
                "over",
                null
              ]
            ],
            "provides": [
              "over"
            ],
            "reads": [
              "amount",
              "budget",
              "c",
              "over",
              "spent",
              "list"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 9,
          "expression": false,
          "source": "let output = {\n  spent = spent,\n  over = over\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "output",
                  "ty": "{ spent: %{string = int}, over: [string] }",
                  "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = []}"
                }
              }
            ]
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 9,
            "diagnostics": "",
            "bindings": [
              [
                "output",
                "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = []}"
              ]
            ],
            "provides": [
              "output"
            ],
            "reads": [],
            "takes": [
              "over",
              "spent"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        }
      ],
      "environment": [
        {
          "name": "budget",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 30000}"
        },
        {
          "name": "category",
          "ty": "fun(string): string",
          "value": "-"
        },
        {
          "name": "output",
          "ty": "{ spent: %{string = int}, over: [string] }",
          "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = []}"
        },
        {
          "name": "over",
          "ty": "[string]",
          "value": null
        },
        {
          "name": "spent",
          "ty": "%{string = int}",
          "value": null
        },
        {
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    },
    {
      "input": "output",
      "outcome": {
        "Unit": {
          "Value": {
            "ty": "{ spent: %{string = int}, over: [string] }",
            "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = []}"
          }
        }
      },
      "units": [
        {
          "number": 1,
          "expression": false,
          "source": "require module sys/std/list",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 1,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "list"
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
          "source": "require module sys/std/map",
          "status": "Ran",
          "outcome": {
            "Ran": []
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 2,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "map"
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
          "source": "require module sys/std/string",
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
            "provides": [
              "string"
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
          "source": "require data local/home/transactions: [\n  {\n    day: u32,\n    payee: string,\n    cents: int,\n  }\n]",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "transactions",
                  "ty": "[{ day: u32, payee: string, cents: int }]",
                  "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
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
                "transactions",
                "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
              ]
            ],
            "provides": [
              "transactions"
            ],
            "reads": [
              "transactions"
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
          "source": "fun category(ref payee: string): string\n  if (\n    string.starts_with(ref payee, ref \"TRADER JOES\")\n    or string.starts_with(ref payee, ref \"SAFEWAY\")\n    or string.starts_with(ref payee, ref \"BI-RITE\")\n  )\n    ret \"groceries\"\n  else if (\n    string.contains(ref payee, ref \"COFFEE\")\n    or string.contains(ref payee, ref \"CAFE\")\n    or string.starts_with(ref payee, ref \"TAQUERIA\")\n  )\n    ret \"dining\"\n  else if string.starts_with(ref payee, ref \"SHELL\")\n    ret \"car\"\n  else if payee == \"PG&E\" or payee == \"NETFLIX.COM\"\n    ret \"bills\"\n  end if\n  ret \"other\"\nend fun",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Function": {
                  "name": "category"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 5,
            "diagnostics": "",
            "bindings": [],
            "provides": [
              "category"
            ],
            "reads": [
              "category",
              "payee",
              "string",
              "string"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 6,
          "expression": false,
          "source": "let budget: %{string = int} = %{\n  \"groceries\" = 30000,\n  \"dining\" = 15000,\n  \"car\" = 10000,\n  \"bills\" = 15000\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "budget",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 30000}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 6,
            "diagnostics": "",
            "bindings": [
              [
                "budget",
                "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 30000}"
              ]
            ],
            "provides": [
              "budget"
            ],
            "reads": [],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 7,
          "expression": false,
          "source": "var spent: %{string = int} = %{}\nfor t in ref transactions\n  let c = category(ref t.payee)\n  let so_far = map.get_or(ref spent, ref c, 0)\n  call map.insert(mut spent, c, so_far + t.cents)\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "spent",
                  "ty": "%{string = int}",
                  "value": "%{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 7,
            "diagnostics": "",
            "bindings": [
              [
                "spent",
                null
              ]
            ],
            "provides": [
              "spent"
            ],
            "reads": [
              "c",
              "category",
              "so_far",
              "spent",
              "t",
              "transactions",
              "map",
              "map"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 8,
          "expression": false,
          "source": "var over: [string] = []\nfor c, amount in ref spent\n  if amount .> budget[c]!\n    call list.push(mut over, c@)\n  end if\nend for",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Slot": {
                  "name": "over",
                  "ty": "[string]",
                  "value": "[]"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 8,
            "diagnostics": "",
            "bindings": [
              [
                "over",
                null
              ]
            ],
            "provides": [
              "over"
            ],
            "reads": [
              "amount",
              "budget",
              "c",
              "over",
              "spent",
              "list"
            ],
            "takes": [],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 9,
          "expression": false,
          "source": "let output = {\n  spent = spent,\n  over = over\n}",
          "status": "Ran",
          "outcome": {
            "Ran": [
              {
                "Value": {
                  "name": "output",
                  "ty": "{ spent: %{string = int}, over: [string] }",
                  "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = []}"
                }
              }
            ]
          },
          "compiled": false,
          "ran": false,
          "detail": {
            "unit": 9,
            "diagnostics": "",
            "bindings": [
              [
                "output",
                "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = []}"
              ]
            ],
            "provides": [
              "output"
            ],
            "reads": [],
            "takes": [
              "over",
              "spent"
            ],
            "overwrites": [],
            "updates": [],
            "revives": []
          }
        },
        {
          "number": 10,
          "expression": true,
          "source": "output",
          "status": "Ran",
          "outcome": {
            "Value": {
              "ty": "{ spent: %{string = int}, over: [string] }",
              "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = []}"
            }
          },
          "compiled": true,
          "ran": true,
          "detail": {
            "unit": 10,
            "diagnostics": "",
            "bindings": [],
            "provides": [],
            "reads": [
              "output"
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
          "name": "budget",
          "ty": "%{string = int}",
          "value": "%{\"bills\" = 15000, \"car\" = 10000, \"dining\" = 15000, \"groceries\" = 30000}"
        },
        {
          "name": "category",
          "ty": "fun(string): string",
          "value": "-"
        },
        {
          "name": "output",
          "ty": "{ spent: %{string = int}, over: [string] }",
          "value": "{spent = %{\"bills\" = 12753, \"car\" = 4650, \"dining\" = 13435, \"groceries\" = 27134}, over = []}"
        },
        {
          "name": "over",
          "ty": "[string]",
          "value": null
        },
        {
          "name": "spent",
          "ty": "%{string = int}",
          "value": null
        },
        {
          "name": "transactions",
          "ty": "[{ day: u32, payee: string, cents: int }]",
          "value": "[{day = 1, payee = \"TRADER JOES #552\", cents = 8412}, {day = 2, payee = \"SHELL OIL 5741\", cents = 4650}, {day = 3, payee = \"BLUE BOTTLE COFFEE\", cents = 675}, {day = 5, payee = \"SAFEWAY 1123\", cents = 6230}, {day = 7, payee = \"TAQUERIA EL FAROLITO\", cents = 2380}, {day = 9, payee = \"PG&E\", cents = 11204}, {day = 12, payee = \"BLUE BOTTLE COFFEE\", cents = 540}, {day = 14, payee = \"TRADER JOES #552\", cents = 9177}, {day = 18, payee = \"NETFLIX.COM\", cents = 1549}, {day = 21, payee = \"ZUNI CAFE\", cents = 9840}, {day = 26, payee = \"BI-RITE MARKET\", cents = 3315}]"
        }
      ]
    }
  ]
});
