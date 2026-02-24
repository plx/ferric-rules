(deffacts initial (bounds  nil))
(defrule Print                     ; DR0336
   (bounds ?type&:(or (eq ?type Cube) (eq ?type Square)))
   =>)
