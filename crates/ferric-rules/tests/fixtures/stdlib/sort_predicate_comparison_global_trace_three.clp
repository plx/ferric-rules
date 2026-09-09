;; #343 pinned sort behavior: comparison-global-trace-three
(defglobal ?*calls* = "" ?*result* = (create$))
(deffunction exchange (?a ?b) (bind ?*calls* (str-cat ?*calls* ?a ":" ?b ";")) (> ?a ?b))
(deffacts startup (go))
(defrule exercise (go) =>
(bind ?*result* (sort exchange (create$ 3 1 2)))
(printout t ?*calls* ":" ?*result* crlf)
)
