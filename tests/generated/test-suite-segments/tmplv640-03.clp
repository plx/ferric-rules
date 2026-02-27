(deftemplate point (slot x) (slot y))
(deffacts start
   (point (x 2) (y 3)))
(defrule munge
   ?p <- (point)
   =>
   (println "(" ?p:x "," ?p:y "," ?p:z ")"))
(defrule munge
   ?p <- (point)
   =>
   (retract ?p)
   (+ ?p:x ?p:y))
