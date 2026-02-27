(defglobal ?*num* = 37)
(defglobal ?*val* = FALSE)
(deffunction get-number ()
   (bind ?*num* (+ ?*num* 1)))
(deffunction muck ()
   (bind ?*val* (create$ (get-number) (get-number))))
(deffacts startup
   (muck-around))
(defrule muck-around
   ?f0 <- (muck-around)
   =>
   (retract ?f0) 
   (muck)
   (assert (muck-around)))
