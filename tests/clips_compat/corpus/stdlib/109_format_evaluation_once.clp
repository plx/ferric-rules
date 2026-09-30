;; #340 pinned CLIPS characterization: evaluation-once
(defglobal ?*trace* = 0)
(deffunction mark (?n ?v) (bind ?*trace* (+ (* ?*trace* 10) ?n)) ?v)
(deffunction fail (?n) (bind ?*trace* (+ (* ?*trace* 10) ?n)) (/ 1 0))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t "result:[" (format (mark 1 nil) (mark 2 "%04d:%s") (mark 3 7) (mark 4 red)) "]" crlf)
(printout t "after" crlf)
(printout t "trace:" ?*trace* crlf)
)
