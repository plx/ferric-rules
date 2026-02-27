(defglobal ?*constraint-salience* = 0)
(defrule bar 
  (declare (salience ?*constraint-salience*))
  ?f <- (x ?y&?x) 
  => 
  (retract ?ins))
