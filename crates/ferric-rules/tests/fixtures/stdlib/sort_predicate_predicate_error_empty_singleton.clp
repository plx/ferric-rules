;; #343 pinned sort behavior: predicate-error-empty-singleton
(defglobal ?*trace* = 0)
(deffunction mark (?n ?v) (bind ?*trace* (+ (* ?*trace* 10) ?n)) ?v)
(deffunction fail (?n) (bind ?*trace* (+ (* ?*trace* 10) ?n)) (/ 1 0))
(deffunction exchange (?a ?b) (fail 3))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t "result:[" (create$ (sort exchange) (sort exchange 7)) "]" crlf)
(printout t "after" crlf)
(printout t "trace:" ?*trace* crlf)
)
