;; #340 pinned CLIPS characterization: empty-format
(defglobal ?*trace* = 0)
(deffunction mark (?n ?v) (bind ?*trace* (+ (* ?*trace* 10) ?n)) ?v)
(deffunction fail (?n) (bind ?*trace* (+ (* ?*trace* 10) ?n)) (/ 1 0))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t "result:[" (format (mark 1 nil) (mark 2 "")) "]" crlf)
(printout t "after" crlf)
(printout t "trace:" ?*trace* crlf)
)
