;; #343 pinned sort behavior: variadic-flatten-empty-singleton

(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort <) ":" (sort < (create$)) ":" (sort < 7) crlf)
(printout t (sort > 3 (create$ 1 4) (create$) 2) crlf)
)
