;; #343 pinned sort behavior: mixed-numeric-types

(deffacts startup (go))
(defrule exercise (go) =>
(progn$ (?x (sort > (create$ 2 1.0 1 2.0 9007199254740993 9007199254740992))) (printout t (integerp ?x) ":" (floatp ?x) ":" ?x crlf))
)
