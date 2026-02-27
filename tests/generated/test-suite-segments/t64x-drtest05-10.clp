(deftemplate a (field one))
(defrule a                         ; DR0453
   ?f1 <- (a (one two three))
   =>
   (assert (not good)))
