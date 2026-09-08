;; #343 pinned sort behavior: comparison-global-trace-five
(defglobal ?*calls* = "" ?*result* = (create$))
(deffunction exchange (?a ?b) (bind ?*calls* (str-cat ?*calls* ?a ":" ?b ";")) (> ?a ?b))
(deffacts startup (go))
(defrule exercise (go) =>
(bind ?*result* (sort exchange (create$ 5 1 4 2 3)))
(printout t ?*calls* ":" ?*result* crlf)
)
