(defglobal ?*queries* = 0)
(defmethod observe ((?x INTEGER (bind ?*queries* (+ ?*queries* 1)))) ?x)
(defrule run =>
  (printout t (observe 2) ":" ?*queries* crlf)
  (printout t (observe 3) ":" ?*queries* crlf))
