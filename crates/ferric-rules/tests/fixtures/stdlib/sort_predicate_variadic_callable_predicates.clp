;; #343 pinned sort behavior: variadic-callable-predicates
(deffunction exchange ($?args) (> (nth$ 1 ?args) (nth$ 2 ?args)))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort + (create$ 3 1 2)) ":" (sort exchange (create$ 3 1 2)) crlf)
)
