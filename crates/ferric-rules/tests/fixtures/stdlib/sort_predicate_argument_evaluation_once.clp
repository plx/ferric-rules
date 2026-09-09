;; #343 pinned sort behavior: argument-evaluation-once
(defglobal ?*trace* = 0)
(deffunction mark (?n ?v) (bind ?*trace* (+ (* ?*trace* 10) ?n)) ?v)
(deffunction fail (?n) (bind ?*trace* (+ (* ?*trace* 10) ?n)) (/ 1 0))

(deffacts startup (go))
(defrule exercise (go) =>
(printout t "result:[" (sort (mark 1 >) (mark 2 (create$ 3 1)) (mark 3 2)) "]" crlf)
(printout t "after" crlf)
(printout t "trace:" ?*trace* crlf)
)
