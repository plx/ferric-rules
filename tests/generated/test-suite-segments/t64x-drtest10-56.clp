(deftemplate example
   (slot value
      (type SYMBOL)
      (allowed-symbols FALSE TRUE)))
(defrule attempt-to-construct-example
   ?f <- (line ?line)
   =>
   (retract ?f)
   (assert (example (value (eq ?line "")))))
