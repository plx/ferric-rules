(deftemplate point (slot x) (slot y))
(defrule count-points
  (point (x ?x) (y ?y))
  =>
  (printout t "Point: " ?x " " ?y crlf))
