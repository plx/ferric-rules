;; #341 pinned nth$ behavior: bad-index-float
(defglobal ?*trace* = 0)
(deffunction mark (?n ?v) (bind ?*trace* (+ (* ?*trace* 10) ?n)) ?v)
(deffunction fail (?n) (bind ?*trace* (+ (* ?*trace* 10) ?n)) (/ 1 0))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t "result:[" (nth$ (mark 1 1.0) (mark 2 (create$ a b))) "]" crlf)
(printout t "after" crlf)
(printout t "trace:" ?*trace* crlf)
)
