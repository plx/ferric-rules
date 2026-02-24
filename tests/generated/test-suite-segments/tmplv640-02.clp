(deftemplate point (slot x) (slot y))
(deftemplate person (slot name) (slot age))
(deffacts start
   (point (x 2) (y 3))
   (person (name "John Smith") (age 53)))
(defrule munge
   ?p <- (point)
   =>
   (println "x = " ?p:x)
   (bind ?p (nth$ 1 (find-fact ((?f person)) TRUE)))
   (println "age = " ?p:age))
