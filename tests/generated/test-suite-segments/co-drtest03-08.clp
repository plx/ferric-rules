(deftemplate a                     ; DR0245
   (field one) (field two))
(defrule b
   (not (a (one anything) (three whatever)))
   =>)
