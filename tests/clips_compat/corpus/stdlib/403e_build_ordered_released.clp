(defrule r =>
  (retract (eval "(assert (p 1))"))
  (printout t (build "(deftemplate p (slot x))") " " (deftemplate-slot-names p) crlf))
