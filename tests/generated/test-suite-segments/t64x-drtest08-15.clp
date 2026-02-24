(defrule fd-1 ; This should be ok
   (a)
   (not (and (b)
             (or (c)
                 (d))))
   =>)
(defrule fd-2 ; this should be ok
   (a)
   (exists (b)
           (or (and (c))
               (d)))
   =>)
(defrule fd-3 ; this should be ok
   (a)
   (not (and (b)
             (or (and (c) (e))
                 (d))))
   =>)
(defrule fd-4 ; this should be ok
   (a)
   (exists (b)
           (or (c)         
               (d)))
   =>)
