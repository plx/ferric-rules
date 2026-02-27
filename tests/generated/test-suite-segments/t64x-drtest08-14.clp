(deftemplate this
   (slot x)
   (slot y (type INTEGER))
   (multislot z (type STRING)))
(defrule this-1 ; This should fail
   (this (x ?x))
   =>
   (member$ a ?x))
