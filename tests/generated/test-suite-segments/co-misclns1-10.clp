(deftemplate A
   (field grid-x)
   (field max)
   (field min-xy)
   (field max-xy))
(defrule p327
  (A (min-xy ?min) (max-xy ?max))
  (A (grid-x ?gx&:(and (>= ?gx ?min) (> ?gx ?max))))
  (A (max ?hmax&:(and (>= ?hmax ?gx) (> ?hmax ?min))))
  =>)
