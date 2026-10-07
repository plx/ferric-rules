(deftemplate p (slot x (allowed-symbols red green)))
(defrule bad (p (x ~blue)) =>)
