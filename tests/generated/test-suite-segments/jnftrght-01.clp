(defrule r1 "error" (exists (logical (a ?) (b ?))) =>)
(defrule r2 "error" (forall (logical (a ?)) (b ?) (c ?)) =>)
(defrule r3 "error" (not (logical (a ?) (b ?))) =>)
